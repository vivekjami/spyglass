//! Spyglass core: the evidence model (README, Evidence Model), configuration,
//! identity, and digests. No I/O beyond loading the config.
//!
//! Everything downstream depends on three properties fixed here:
//!   * an `Event` has a stable `event_id` and a `template_id` derived from a
//!     masked message, so "the same kind of line" has one identity
//!   * a digest of a result is computed over canonical JSON with evidence ids
//!     stripped, so identical evidence yields identical digests regardless of
//!     which investigation numbered it (ADR-004)
//!   * a `LedgerEntry` carries the *resolved* arguments, not just their hash,
//!     so a citation can be re-executed later (ADR-009)

use std::{collections::BTreeMap, path::PathBuf, sync::LazyLock};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

// ------------------------------------------------------------------ config

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub paths: Paths,
    pub bounds: Bounds,
    pub windows: Windows,
    pub ingest: Ingest,
    pub services: Vec<ServiceCfg>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Paths {
    pub log_dir: PathBuf,
    pub deploy_dir: PathBuf,
    pub segment_dir: PathBuf,
    pub ledger_dir: PathBuf,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Bounds {
    pub max_items: usize,
    pub search_limit_max: usize,
    pub max_bytes_per_item: usize,
    pub excerpt_bytes: usize,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Windows {
    pub default_lookback_secs: i64,
    pub deploy_correlation_secs: i64,
    pub clock_skew_tolerance_secs: i64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Ingest {
    pub poll_ms: u64,
    pub metrics_scrape_secs: u64,
    pub metrics_ring: usize,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ServiceCfg {
    pub name: String,
    pub service: String,
    pub role: String,
    #[serde(default)]
    pub upstreams: Vec<String>,
    pub port_env: Option<String>,
    pub default_port: Option<u16>,
    #[serde(default)]
    pub has_log: bool,
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Ok(toml::from_str(&text)?)
    }

    pub fn metrics_url(&self, svc: &ServiceCfg) -> Option<String> {
        let port = svc
            .port_env
            .as_deref()
            .and_then(|v| std::env::var(v).ok())
            .and_then(|v| v.parse::<u16>().ok())
            .or(svc.default_port)?;
        Some(format!("http://127.0.0.1:{port}/metrics"))
    }
}

// ------------------------------------------------------------------ windows

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

impl Window {
    pub fn contains(&self, ts: DateTime<Utc>) -> bool {
        ts >= self.from && ts <= self.to
    }
    pub fn ending_at(to: DateTime<Utc>, lookback_secs: i64) -> Self {
        Self { from: to - Duration::seconds(lookback_secs), to }
    }
}

// ------------------------------------------------------------------ events

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Event {
    /// `<instance>:<line number>` -- stable for the life of the log file.
    pub event_id: String,
    pub ts: DateTime<Utc>,
    pub service: String,
    pub instance: String,
    pub version: String,
    pub level: String,
    pub req_id: Option<String>,
    pub msg: String,
    pub route: Option<String>,
    pub status: Option<u16>,
    pub latency_ms: Option<f64>,
    pub deploy_id: Option<String>,
    pub upstream: Option<String>,
    pub kind: Option<String>,
    pub has_stack: bool,
    pub template_id: String,
    pub pattern: String,
    /// The original line, capped. Data, never instructions.
    pub raw: String,
}

impl Event {
    /// Parse one JSON log line. Returns None for anything that is not a
    /// well-formed event with a parseable `ts` -- the caller counts those as
    /// malformed rather than crashing (README C1: never crash on input).
    pub fn parse(raw: &str, instance_hint: &str, event_id: String, raw_cap: usize) -> Option<Event> {
        let v: Value = serde_json::from_str(raw).ok()?;
        let ts = v.get("ts")?.as_str()?.parse::<DateTime<Utc>>().ok()?;
        let msg = v.get("msg")?.as_str()?.to_string();
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
        let pattern = mask(&msg);
        let mut raw_capped = raw.to_string();
        if raw_capped.len() > raw_cap {
            raw_capped.truncate(raw_cap);
            raw_capped.push_str("…[capped]");
        }
        Some(Event {
            event_id,
            ts,
            service: s("service").unwrap_or_else(|| instance_hint.to_string()),
            instance: s("instance").unwrap_or_else(|| instance_hint.to_string()),
            version: s("version").unwrap_or_default(),
            level: s("level").unwrap_or_else(|| "INFO".into()),
            req_id: s("req_id"),
            route: s("route"),
            status: v.get("status").and_then(|x| x.as_u64()).map(|x| x as u16),
            latency_ms: v.get("latency_ms").and_then(|x| x.as_f64()),
            deploy_id: s("deploy_id"),
            upstream: s("upstream"),
            kind: s("kind"),
            has_stack: v.get("stack").is_some(),
            template_id: template_id(&pattern),
            pattern,
            msg,
            raw: raw_capped,
        })
    }
}

/// One line of the deployer's journal.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeployEvent {
    pub n: u64,
    pub kind: String,
    #[serde(default)]
    pub deploy_id: Option<String>,
    pub service: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub from_version: Option<String>,
    pub ts: DateTime<Utc>,
    pub actor: String,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub justification_eids: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

// ------------------------------------------------------------------ masking + identity

static MASKS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").unwrap(), "<*>"),
        (Regex::new(r"\d{4}-\d{2}-\d{2}T[\d:.]+Z?").unwrap(), "<*>"),
        (Regex::new(r"\b[0-9a-f]{8,}\b").unwrap(), "<*>"),
        (Regex::new(r"\b[A-Za-z]+_[0-9a-f]{8,}\b").unwrap(), "<*>"),
        (Regex::new(r"\b\d+(\.\d+)?\b").unwrap(), "<*>"),
        (Regex::new(r"\b[A-Z]{3}\b").unwrap(), "<*>"),
    ]
});

/// Replace variable parts of a log message with `<*>` so lines that differ
/// only in ids, numbers, currencies, or timestamps share one template. This is
/// the Phase 3 stepping stone to Drain (Phase 4): masking only, no tree.
pub fn mask(msg: &str) -> String {
    let mut s = msg.to_string();
    for (re, rep) in MASKS.iter() {
        s = re.replace_all(&s, *rep).into_owned();
    }
    s
}

pub fn template_id(pattern: &str) -> String {
    format!("T-{}", &sha256_hex(pattern.as_bytes())[..12])
}

// ------------------------------------------------------------------ digests

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex(&h.finalize())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Canonical digest of a JSON value: serde_json's Map is a BTreeMap, so keys
/// serialise sorted; evidence ids are stripped first because they are
/// assigned per investigation and must not perturb the digest (ADR-004).
pub fn digest_json(v: &Value) -> String {
    let stripped = strip_eids(v);
    sha256_hex(serde_json::to_string(&stripped).unwrap_or_default().as_bytes())
}

fn strip_eids(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter().filter(|(k, _)| k.as_str() != "eid").map(|(k, v)| (k.clone(), strip_eids(v))).collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(strip_eids).collect()),
        other => other.clone(),
    }
}

/// Serialised size cap for any single evidence item (ADR-005). Items over the
/// cap have their largest string field truncated until they fit.
pub fn cap_item(item: &mut Value, max_bytes: usize) {
    for _ in 0..8 {
        if serde_json::to_vec(item).map(|b| b.len()).unwrap_or(0) <= max_bytes {
            return;
        }
        let Value::Object(m) = item else { return };
        let Some((k, len)) = m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.len()))).max_by_key(|(_, l)| *l) else { return };
        if len < 64 {
            return;
        }
        if let Some(Value::String(s)) = m.get_mut(&k) {
            s.truncate(len / 2);
            s.push_str("…[capped]");
        }
    }
}

// ------------------------------------------------------------------ ledger

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LedgerEntry {
    pub n: u64,
    pub ts: String,
    pub investigation: String,
    pub tool: String,
    /// Resolved arguments (windows filled in), so the call can be re-executed.
    pub args: Value,
    pub args_hash: String,
    pub result_digest: String,
    pub eids: Vec<String>,
    pub summary: String,
    pub latency_ms: f64,
    pub deterministic: bool,
}

/// Metadata attached to every tool response (README, MCP Interface rule 4).
#[derive(Serialize, Clone, Debug)]
pub struct Meta {
    pub investigation: String,
    pub eids: Vec<String>,
    pub query_hash: String,
    pub result_digest: String,
    pub window: Option<Window>,
    pub watermark: BTreeMap<String, DateTime<Utc>>,
    pub lag_ms: i64,
    pub engine_latency_ms: f64,
    pub deterministic: bool,
    pub bounds: BoundsApplied,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct BoundsApplied {
    pub max_items: usize,
    pub items_returned: usize,
    pub items_available: usize,
    pub truncated: bool,
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
