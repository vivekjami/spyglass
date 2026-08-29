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
    pub drain: DrainCfg,
    pub novelty: NoveltyCfg,
    pub changepoints: ChangepointCfg,
    pub ranking: RankingCfg,
    pub bundle: BundleCfg,
    /// README C9 / ADR-010: the causal replay, as built in Phase 8.
    #[serde(default)]
    pub replay: ReplayCfg,
    /// README C11: the post-action verification loop, as built in Phase 9.
    #[serde(default)]
    pub verify: VerifyCfg,
    /// Engine-side backstop against runaway agents (README, Safety Model).
    #[serde(default)]
    pub limits: LimitsCfg,
    pub services: Vec<ServiceCfg>,
    /// Set by the server's `--ablation` flag, never by the file: names the
    /// ablation this engine instance runs under, stamped on the watermark.
    #[serde(default)]
    pub ablation: Option<String>,
}

fn default_true() -> bool {
    true
}

/// README C11. The engine judges recovery; the agent only asks.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct VerifyCfg {
    /// Suggested gap between checks (the SOP sleeps this long).
    pub interval_secs: i64,
    /// Consecutive clean checks that close the incident.
    pub checks_required: u32,
    /// Seconds after the action before a still-open verification escalates.
    pub timeout_secs: i64,
    /// The post-action window each check judges: the last N seconds of ingested data after the action.
    pub window_secs: i64,
    /// Pre-incident baseline: the N seconds before the incident began.
    pub baseline_secs: i64,
    /// When the journal cannot name the deploy the action reverted, the incident is taken to be this long before the action.
    pub incident_lookback_secs: i64,
    /// Clean = post rate <= max(baseline * tolerance_ratio, baseline + tolerance_abs).
    pub tolerance_abs: f64,
    pub tolerance_ratio: f64,
    /// Fewer request events than this in the post window = insufficient data, not a verdict.
    pub min_requests: u64,
}

impl Default for VerifyCfg {
    fn default() -> Self {
        Self { interval_secs: 15, checks_required: 2, timeout_secs: 300, window_secs: 60, baseline_secs: 300, incident_lookback_secs: 300, tolerance_abs: 0.02, tolerance_ratio: 1.5, min_requests: 20 }
    }
}

/// Per-investigation call budget, enforced by the engine regardless of prompt.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct LimitsCfg {
    pub max_calls_per_investigation: u64,
    pub max_calls_per_minute: u64,
}

impl Default for LimitsCfg {
    fn default() -> Self {
        Self { max_calls_per_investigation: 200, max_calls_per_minute: 60 }
    }
}

/// The exemplar replay (README, Sandbox Causal Verification; ADR-010).
/// Bounds are engine-enforced: `n` is clamped to `max_n`, every request
/// times out, bodies are capped, and the traffic is tagged so the engine
/// can keep its own experiment out of the evidence.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ReplayCfg {
    /// Replays per version when the call does not say.
    pub default_n: usize,
    /// Hard cap on replays per version.
    pub max_n: usize,
    /// Per-request timeout.
    pub timeout_ms: u64,
    /// Cap on a sanitized request body, in bytes.
    pub body_cap: usize,
    /// Failure-proportion gap between the best and worst version at or above
    /// which the experiment is reported as `separated`.
    pub separation_min_delta: f64,
    /// Where a captured edge request is replayed for a given service.
    #[serde(default)]
    pub routes: Vec<ReplayRoute>,
}

impl Default for ReplayCfg {
    fn default() -> Self {
        Self { default_n: 20, max_n: 50, timeout_ms: 3000, body_cap: 2048, separation_min_delta: 0.5, routes: vec![] }
    }
}

/// "A request captured at `captured_path` is replayed against `service`'s
/// version instances at `path`." The captured body is forwarded as-is.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ReplayRoute {
    pub captured_path: String,
    pub service: String,
    pub path: String,
}

/// README C5 / ADR-008: the hand-weighted linear ranking model. Weights are
/// opinions -- stated here, tuned on S1, inspectable in every ledger entry.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct RankingCfg {
    pub w_n: f64,
    pub w_t: f64,
    pub w_s: f64,
    pub w_d: f64,
    pub w_f: f64,
    pub w_r: f64,
    /// Temporal proximity decays as exp(-|t - T0| / tau).
    pub proximity_tau_secs: f64,
    /// Service relevance decays by this factor per topology hop from the focus service.
    pub relevance_hop_decay: f64,
    /// Error changepoints / error templates within this of each other on connected services are one cascade.
    pub cascade_secs: f64,
}

/// README C6: bundle bounds, enforced by the engine.
#[derive(Deserialize, Clone, Debug)]
pub struct BundleCfg {
    pub max_bytes: usize,
    pub incident_window_secs: i64,
    pub baseline_secs: i64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct DrainCfg {
    pub depth: usize,
    pub similarity_threshold: f64,
    pub max_children: usize,
}

#[derive(Deserialize, Clone, Debug)]
pub struct NoveltyCfg {
    /// Ablation A1 (`--ablation no-novelty`): false disables `novel_templates` and drops
    /// template candidates from the bundle. Config default: true.
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub incident_window_secs: i64,
    pub baseline_secs: i64,
    pub warmup_secs: i64,
    pub min_baseline_secs: i64,
    pub burst_log2_scale: f64,
    pub severity_boost: f64,
    pub min_score: f64,
}

/// README C4 / ADR-007. Every threshold the detector uses lives here, tuned
/// on S1 only (Phase 5) and said so.
#[derive(Deserialize, Clone, Debug)]
pub struct ChangepointCfg {
    pub bucket_secs: i64,
    pub z_threshold: f64,
    pub consecutive_buckets: usize,
    pub baseline_secs: i64,
    pub guard_secs: i64,
    pub min_baseline_buckets: usize,
    pub sigma_floor_count: f64,
    pub sigma_floor_rate: f64,
    pub sigma_floor_latency_frac: f64,
    pub sigma_floor_latency_ms: f64,
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
    /// The version this instance runs, for services with always-on version
    /// pairs (ADR-017): the replay's targets are the instances that carry one.
    #[serde(default)]
    pub version: Option<String>,
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Ok(toml::from_str(&text)?)
    }

    /// The instance's host-published base URL: the same env vars Compose
    /// uses, so the engine hits what `just up` published.
    pub fn base_url(&self, svc: &ServiceCfg) -> Option<String> {
        let port = svc
            .port_env
            .as_deref()
            .and_then(|v| std::env::var(v).ok())
            .and_then(|v| v.parse::<u16>().ok())
            .or(svc.default_port)?;
        Some(format!("http://127.0.0.1:{port}"))
    }

    pub fn metrics_url(&self, svc: &ServiceCfg) -> Option<String> {
        self.base_url(svc).map(|b| format!("{b}/metrics"))
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

// ------------------------------------------------------------------ sanitization

/// Header names that never leave the capture, whatever the gateway kept
/// (README, Safety Model: "exemplar sanitization strips auth headers and
/// caps bodies regardless, because the pattern must survive contact with
/// real data someday"). Matched on the lowercased name: exact, or any name
/// containing one of the fragments.
const DROP_HEADER_EXACT: &[&str] = &["authorization", "proxy-authorization", "cookie", "set-cookie", "x-api-key", "api-key", "x-auth-token", "x-csrf-token"];
const DROP_HEADER_FRAGMENTS: &[&str] = &["auth", "token", "secret", "session", "cookie", "credential", "password", "signature"];
const HEADER_VALUE_CAP: usize = 256;

/// JSON keys whose values are secret-shaped, compared after lowercasing and
/// removing `-`/`_`; plus any key ending in one of the suffixes.
const REDACT_KEYS: &[&str] = &[
    "password", "passwd", "pwd", "secret", "token", "accesstoken", "refreshtoken", "idtoken", "apikey", "authorization", "auth",
    "cvv", "cvc", "cvv2", "cardnumber", "pan", "ssn", "privatekey", "accesskey", "secretkey", "clientsecret", "otp", "pin",
];
const REDACT_SUFFIXES: &[&str] = &["token", "secret", "password", "apikey"];

static PAN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:\d[ -]?){12,18}\d\b").unwrap());

pub fn header_is_dropped(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    DROP_HEADER_EXACT.contains(&n.as_str()) || DROP_HEADER_FRAGMENTS.iter().any(|f| n.contains(f))
}

fn key_is_secret(key: &str) -> bool {
    let k: String = key.to_ascii_lowercase().chars().filter(|c| *c != '-' && *c != '_' && *c != ' ').collect();
    REDACT_KEYS.contains(&k.as_str()) || REDACT_SUFFIXES.iter().any(|s| k.len() > s.len() && k.ends_with(s))
}

/// Keep only headers that are not auth-shaped; cap every value. Returns the
/// kept map (names lowercased, sorted) and the names that were dropped.
pub fn sanitize_headers(headers: &Value) -> (BTreeMap<String, String>, Vec<String>) {
    let mut kept = BTreeMap::new();
    let mut dropped = Vec::new();
    if let Value::Object(m) = headers {
        for (k, v) in m {
            let name = k.to_ascii_lowercase();
            if header_is_dropped(&name) {
                dropped.push(name);
                continue;
            }
            let mut val = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            if val.len() > HEADER_VALUE_CAP {
                let mut cut = HEADER_VALUE_CAP;
                while !val.is_char_boundary(cut) {
                    cut -= 1;
                }
                val.truncate(cut);
                val.push_str("…[capped]");
            }
            kept.insert(name, val);
        }
    }
    dropped.sort();
    (kept, dropped)
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SanitizedBody {
    pub text: String,
    pub bytes: usize,
    pub truncated: bool,
    /// What was redacted, by JSON path (or "text" for a non-JSON body).
    pub redactions: Vec<String>,
}

fn redact_value(v: &mut Value, path: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, val) in m.iter_mut() {
                let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                if key_is_secret(k) {
                    *val = Value::String("[redacted]".into());
                    out.push(p);
                } else {
                    redact_value(val, &p, out);
                }
            }
        }
        Value::Array(a) => {
            for (i, val) in a.iter_mut().enumerate() {
                redact_value(val, &format!("{path}[{i}]"), out);
            }
        }
        Value::String(s) => {
            if PAN_RE.is_match(s) {
                *s = PAN_RE.replace_all(s, "[redacted:pan]").into_owned();
                out.push(format!("{path}:pan"));
            }
        }
        _ => {}
    }
}

/// Redact secret-shaped JSON keys and card-like digit runs, then cap. A body
/// with nothing to redact is returned byte-identical (up to the cap), so a
/// replay sends what was captured.
pub fn sanitize_body(body: &str, cap: usize) -> SanitizedBody {
    let mut redactions = Vec::new();
    let mut text = match serde_json::from_str::<Value>(body) {
        Ok(mut v) => {
            redact_value(&mut v, "", &mut redactions);
            if redactions.is_empty() { body.to_string() } else { v.to_string() }
        }
        Err(_) => {
            if PAN_RE.is_match(body) {
                redactions.push("text:pan".into());
                PAN_RE.replace_all(body, "[redacted:pan]").into_owned()
            } else {
                body.to_string()
            }
        }
    };
    let bytes = text.len();
    let truncated = bytes > cap;
    if truncated {
        let mut cut = cap;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("…[capped]");
    }
    SanitizedBody { text, bytes, truncated, redactions }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn auth_shaped_headers_are_dropped_and_the_rest_kept_lowercased() {
        let h = json!({"Content-Type": "application/json", "Authorization": "Bearer x", "Cookie": "a=b",
                       "X-Client-Class": "premium", "X-Auth-Token": "t", "X-Idempotency-Key": "k", "User-Agent": "ua"});
        let (kept, dropped) = sanitize_headers(&h);
        assert_eq!(kept.keys().cloned().collect::<Vec<_>>(), vec!["content-type", "user-agent", "x-client-class", "x-idempotency-key"]);
        assert_eq!(dropped, vec!["authorization", "cookie", "x-auth-token"]);
    }

    #[test]
    fn header_values_are_capped_on_a_char_boundary() {
        let long = "é".repeat(300);
        let (kept, _) = sanitize_headers(&json!({"user-agent": long}));
        let v = &kept["user-agent"];
        assert!(v.ends_with("…[capped]"));
        assert!(v.len() <= HEADER_VALUE_CAP + "…[capped]".len());
    }

    #[test]
    fn a_clean_body_is_returned_byte_identical() {
        let body = r#"{"currency":"EUR","customer":"cust-7","card_class":"premium","amount":42.1}"#;
        let s = sanitize_body(body, 2048);
        assert_eq!(s.text, body);
        assert!(s.redactions.is_empty());
        assert!(!s.truncated);
    }

    #[test]
    fn secret_keys_and_card_numbers_are_redacted_by_path() {
        let body = r#"{"customer":"c","card":{"number":"4111 1111 1111 1111","cvv":"123"},"password":"hunter2","session_token":"abc"}"#;
        let s = sanitize_body(body, 2048);
        assert!(!s.text.contains("4111"));
        assert!(!s.text.contains("hunter2"));
        assert!(!s.text.contains("\"abc\""));
        let mut r = s.redactions.clone();
        r.sort();
        assert_eq!(r, vec!["card.cvv", "card.number:pan", "password", "session_token"]);
    }

    #[test]
    fn non_json_bodies_still_lose_card_like_runs_and_get_capped() {
        let s = sanitize_body("pan=4111111111111111&x=1", 8);
        assert!(s.text.starts_with("pan=[red"));
        assert!(s.truncated);
        assert_eq!(s.redactions, vec!["text:pan"]);
    }

    #[test]
    fn amounts_and_ids_are_not_mistaken_for_card_numbers() {
        let body = r#"{"amount":178.91,"order":"ord_1234567890ab","req":"d49edd91-2f1f-4132-826b-2236e4a07521"}"#;
        assert!(sanitize_body(body, 2048).redactions.is_empty());
    }
}
