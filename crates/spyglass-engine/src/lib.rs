//! The evidence engine: an in-memory store fed by tailers and a scraper, the
//! evidence tools over it, and per-investigation evidence ids + ledger.
//!
//! Phase 3 shape ("ugly but complete"): one crate, three modules. The store
//! is rebuilt from the source log files on start; segment files are written
//! as a durable copy but not yet read back. Templates are masking-based
//! then routed through a Drain tree (Phase 4) that owns template identity.
//! Changepoints (Phase 5) are detected on request series derived from the
//! events -- event-time stamped and rebuilt on start, so deterministic; the
//! scraped Prometheus counters are ingested and watermarked for freshness
//! (ADR-007 says why they are not the detector's input). Ranking (Phase 6)
//! and bundles (Phase 7) sit on top of the tools: one scored, deduped,
//! kind-diverse, byte-bounded list. The causal check (Phase 8) is the one
//! tool that touches the world: it replays a captured request against the
//! always-on version instances and keeps its own traffic out of the store.

pub mod bundle;
pub mod changepoints;
pub mod drain;
pub mod ingest;
pub mod rank;
pub mod replay;
pub mod tools;
pub mod verify;

use std::path::Path;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fs,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use spyglass_core::{Config, DeployEvent, Event, LedgerEntry, Window};

// ------------------------------------------------------------------ store

#[derive(Serialize, Clone, Debug)]
pub struct Template {
    pub template_id: String,
    pub pattern: String,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub count: u64,
    pub level_hist: BTreeMap<String, u64>,
    pub services: BTreeSet<String>,
    pub instances: BTreeSet<String>,
    /// Indices into `Store::events` of the first few examples.
    pub example_idx: Vec<usize>,
}

pub struct Store {
    /// Template identity lives here: masked message -> Drain cluster.
    pub drain: drain::Drain,
    pub events: Vec<Event>,
    pub by_event_id: HashMap<String, usize>,
    pub templates: HashMap<String, Template>,
    pub deploys: Vec<DeployEvent>,
    /// series key -> ring of (ts, value)
    pub metrics: HashMap<String, VecDeque<(DateTime<Utc>, f64)>>,
    /// "log:<instance>" | "journal" | "metrics" -> newest timestamp seen
    pub watermarks: BTreeMap<String, DateTime<Utc>>,
    /// req_id -> index of the gateway's `request_capture` event for it:
    /// the request as the client sent it, for `get_exemplar_request`.
    pub captures: HashMap<String, usize>,
    pub ingested: u64,
    pub malformed: u64,
    /// Log lines produced by the engine's own replay experiments (req_id
    /// `replay-*`), dropped at ingest so the evidence never contains the
    /// investigation's own traffic.
    pub replay_lines_excluded: u64,
    /// Earliest event timestamp in the store: the start of known history.
    /// Novelty is only claimable for templates that appeared after it.
    pub earliest_ts: Option<DateTime<Utc>>,
    /// Bumped whenever a source file shrinks (the stack was reset); the
    /// store is cleared so evidence from a previous incident cannot leak in.
    pub epoch: u64,
    drain_cfg: spyglass_core::DrainCfg,
}

impl Store {
    pub fn new(drain_cfg: spyglass_core::DrainCfg) -> Self {
        Self {
            drain: drain::Drain::new(drain_cfg.clone()),
            events: Vec::new(),
            by_event_id: HashMap::new(),
            templates: HashMap::new(),
            deploys: Vec::new(),
            metrics: HashMap::new(),
            watermarks: BTreeMap::new(),
            captures: HashMap::new(),
            ingested: 0,
            malformed: 0,
            replay_lines_excluded: 0,
            earliest_ts: None,
            epoch: 0,
            drain_cfg,
        }
    }

    pub fn append(&mut self, mut e: Event) {
        let idx = self.events.len();
        // Drain assigns identity from the masked message, keyed by log level:
        // "request completed" (INFO) and "request failed" (ERROR) share one of
        // two tokens and would otherwise merge at the 0.5 threshold -- a
        // distinction no investigator wants erased. The level routes the tree;
        // similarity is measured over message tokens only.
        let tokens: Vec<String> = e.pattern.split_whitespace().map(str::to_string).collect();
        let (cid, _created) = self.drain.insert_keyed(&e.level, tokens);
        e.template_id = format!("T{cid}");
        e.pattern = self
            .drain
            .cluster(cid)
            .map(|c| c.template())
            .unwrap_or_else(|| e.pattern.clone());
        self.earliest_ts = Some(self.earliest_ts.map_or(e.ts, |t| t.min(e.ts)));
        let t = self
            .templates
            .entry(e.template_id.clone())
            .or_insert_with(|| Template {
                template_id: e.template_id.clone(),
                pattern: e.pattern.clone(),
                first_seen: e.ts,
                last_seen: e.ts,
                count: 0,
                level_hist: BTreeMap::new(),
                services: BTreeSet::new(),
                instances: BTreeSet::new(),
                example_idx: vec![],
            });
        t.pattern = e.pattern.clone(); // a merge may have added wildcards since
        t.count += 1;
        t.first_seen = t.first_seen.min(e.ts);
        t.last_seen = t.last_seen.max(e.ts);
        *t.level_hist.entry(e.level.clone()).or_default() += 1;
        t.services.insert(e.service.clone());
        t.instances.insert(e.instance.clone());
        if t.example_idx.len() < 3 {
            t.example_idx.push(idx);
        }
        self.watermarks
            .entry(format!("log:{}", e.instance))
            .and_modify(|w| *w = (*w).max(e.ts))
            .or_insert(e.ts);
        self.by_event_id.insert(e.event_id.clone(), idx);
        if e.kind.as_deref() == Some("request_capture")
            && let Some(r) = &e.req_id
        {
            self.captures.entry(r.clone()).or_insert(idx);
        }
        self.ingested += 1;
        self.events.push(e);
    }

    pub fn reset(&mut self) {
        let epoch = self.epoch + 1;
        *self = Store::new(self.drain_cfg.clone());
        self.epoch = epoch;
    }

    /// Newest log timestamp across every instance -- the "now" of the
    /// evidence, as opposed to wall-clock now.
    pub fn newest_log_ts(&self) -> Option<DateTime<Utc>> {
        self.watermarks
            .iter()
            .filter(|(k, _)| k.starts_with("log:"))
            .map(|(_, v)| *v)
            .max()
    }

    /// The newest timestamp every ACTIVE log source has been read past: a
    /// line with ts <= this has been ingested from every file that is still
    /// writing (per-file timestamps are monotonic). Windows resolve their
    /// end here rather than at `newest_log_ts`, so a query never includes a
    /// bucket that another file has not delivered yet -- that is what makes
    /// "deterministic on frozen data" true at the live edge (ADR-004). A
    /// source more than `ACTIVE_SECS` behind the newest is idle (a version
    /// with no traffic) and does not hold the watermark back.
    pub fn safe_log_ts(&self) -> Option<DateTime<Utc>> {
        const ACTIVE_SECS: i64 = 5;
        let newest = self.newest_log_ts()?;
        let cutoff = newest - chrono::Duration::seconds(ACTIVE_SECS);
        self.watermarks
            .iter()
            .filter(|(k, _)| k.starts_with("log:"))
            .map(|(_, v)| *v)
            .filter(|v| *v >= cutoff)
            .min()
    }

    pub fn events_in<'a>(
        &'a self,
        w: &'a Window,
        services: &'a [String],
    ) -> impl Iterator<Item = &'a Event> + 'a {
        self.events
            .iter()
            .filter(move |e| w.contains(e.ts))
            .filter(move |e| {
                services.is_empty() || services.iter().any(|s| *s == e.service || *s == e.instance)
            })
    }
}

// ------------------------------------------------------------------ investigations

/// One investigation = one MCP session. Owns the evidence-id counter, the
/// evidence records those ids dereference to, and the ledger file.
pub struct Investigation {
    pub id: String,
    pub next_n: u64,
    pub next_eid: u64,
    pub evidence: BTreeMap<String, Value>,
    pub ledger_path: PathBuf,
    pub evidence_path: PathBuf,
    /// Post-action verification state per "<service>:<deploy_id>" (Phase 9).
    pub verifications: BTreeMap<String, verify::VerifyState>,
    /// Engine-side call budget (README, Safety Model: the backstop).
    pub calls_total: u64,
    pub recent_calls: VecDeque<std::time::Instant>,
}

impl Investigation {
    fn new(dir: &Path, id: &str) -> Self {
        let safe: String = id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        Self {
            id: id.to_string(),
            next_n: 1,
            next_eid: 1,
            evidence: BTreeMap::new(),
            ledger_path: dir.join(format!("{safe}.jsonl")),
            evidence_path: dir.join(format!("{safe}.evidence.jsonl")),
            verifications: BTreeMap::new(),
            calls_total: 0,
            recent_calls: VecDeque::new(),
        }
    }

    /// Admit one more tool call under the budget, or say why not. Enforced
    /// here so no prompt can talk the engine out of it; exhaustion is a
    /// tool error the agent sees and must synthesise around.
    pub fn admit(&mut self, limits: &spyglass_core::LimitsCfg) -> std::result::Result<(), String> {
        let now = std::time::Instant::now();
        while self
            .recent_calls
            .front()
            .is_some_and(|t| now.duration_since(*t).as_secs() >= 60)
        {
            self.recent_calls.pop_front();
        }
        if self.calls_total >= limits.max_calls_per_investigation {
            return Err(format!(
                "budget exhausted: {} tool calls in this investigation (limit {}); synthesise from the evidence you have and label the report partial",
                self.calls_total, limits.max_calls_per_investigation
            ));
        }
        if self.recent_calls.len() as u64 >= limits.max_calls_per_minute {
            return Err(format!(
                "rate limit: {} tool calls in the last minute (limit {}); stop and synthesise from the evidence you have",
                self.recent_calls.len(),
                limits.max_calls_per_minute
            ));
        }
        self.calls_total += 1;
        self.recent_calls.push_back(now);
        Ok(())
    }

    /// Assign the next `E<n>` to an evidence record and persist it.
    pub fn issue_eid(&mut self, mut item: Value) -> String {
        let eid = format!("E{}", self.next_eid);
        self.next_eid += 1;
        if let Value::Object(m) = &mut item {
            m.insert("eid".into(), Value::String(eid.clone()));
        }
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.evidence_path)
        {
            let _ = writeln!(f, "{}", item);
        }
        self.evidence.insert(eid.clone(), item);
        eid
    }

    pub fn record(&mut self, mut entry: LedgerEntry) -> Result<LedgerEntry> {
        entry.n = self.next_n;
        entry.investigation = self.id.clone();
        self.next_n += 1;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.ledger_path)?;
        writeln!(f, "{}", serde_json::to_string(&entry)?)?;
        Ok(entry)
    }
}

// ------------------------------------------------------------------ engine

pub struct Engine {
    pub cfg: Config,
    pub store: RwLock<Store>,
    pub investigations: Mutex<HashMap<String, Investigation>>,
    pub started: DateTime<Utc>,
    /// False while the tailer is still rebuilding the store from the files
    /// after a start or a reset; true once every file has been read to its
    /// end at least once. A query issued before that sees a partial world.
    pub caught_up: std::sync::atomic::AtomicBool,
    /// For the replay experiment (and nothing else).
    pub http: reqwest::Client,
}

impl Engine {
    pub fn new(cfg: Config) -> Arc<Self> {
        fs::create_dir_all(&cfg.paths.ledger_dir).ok();
        fs::create_dir_all(&cfg.paths.segment_dir).ok();
        let store = Store::new(cfg.drain.clone());
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(cfg.replay.timeout_ms))
            .build()
            .expect("http client");
        Arc::new(Self {
            cfg,
            store: RwLock::new(store),
            investigations: Mutex::new(HashMap::new()),
            started: Utc::now(),
            caught_up: std::sync::atomic::AtomicBool::new(false),
            http,
        })
    }

    /// Start the tailers and the scraper. Log/journal tailing runs on plain
    /// threads (blocking file I/O); the scraper is an async task.
    pub fn start(self: &Arc<Self>) {
        ingest::spawn(self.clone());
    }

    pub fn with_investigation<T>(&self, id: &str, f: impl FnOnce(&mut Investigation) -> T) -> T {
        let mut m = self.investigations.lock().expect("investigations lock");
        let inv = m
            .entry(id.to_string())
            .or_insert_with(|| Investigation::new(&self.cfg.paths.ledger_dir, id));
        f(inv)
    }

    pub fn watermarks(&self) -> (BTreeMap<String, DateTime<Utc>>, i64) {
        let s = self.store.read().expect("store lock");
        let lag = s
            .newest_log_ts()
            .map(|t| (Utc::now() - t).num_milliseconds())
            .unwrap_or(-1);
        (s.watermarks.clone(), lag)
    }
}
