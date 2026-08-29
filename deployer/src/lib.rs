//! Spyglass deployer: the control plane for the target system.
//!
//! This is the *write plane*. The evidence engine never touches it. It owns
//! two files under a data directory (bind-mounted read-only into the services):
//!
//!   current.json   which version each service is routed to (orders reads it
//!                  per request, so a switch takes effect with no restart)
//!   journal.jsonl  append-only record of every deploy / rollback / no-op --
//!                  the highest-prior evidence class, tailed by the engine
//!
//! `deploy` is scenario tooling and is never exposed to the agent. `rollback`
//! is the one mutating action the agent may propose: idempotent on
//! `request_id`, and it refuses to act if the world moved since the proposal
//! (`expected_current`). The CLI (main.rs) and the MCP tool (`serve`) both
//! call the functions here, so the gate wraps one tested implementation.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Every version that exists as a runnable artifact. A deploy to anything
/// else is a typo, not a deployment.
pub const KNOWN_VERSIONS: &[(&str, &[&str])] = &[
    ("gateway", &["v1"]),
    ("orders", &["v1", "v1.1"]),
    ("payments", &["v1", "v2"]),
];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServiceState {
    pub version: String,
    pub deploy_id: Option<String>,
    pub since: String,
}

pub type State = BTreeMap<String, ServiceState>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Entry {
    pub n: u64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy_id: Option<String>,
    pub service: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_version: Option<String>,
    pub ts: String,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub justification_eids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackOutcome {
    /// Routing changed; a new deploy id was issued.
    Executed,
    /// Nothing changed: duplicate request_id, or already at the version.
    Noop,
    /// Refused: the current version was not the one the proposal named.
    Aborted,
}

pub struct Store {
    pub state_path: PathBuf,
    pub journal_path: PathBuf,
}

impl Store {
    pub fn new(dir: &Path) -> Self {
        Self { state_path: dir.join("current.json"), journal_path: dir.join("journal.jsonl") }
    }

    pub fn load_state(&self) -> Result<State> {
        let s = fs::read_to_string(&self.state_path)
            .with_context(|| format!("read {}; run `deployer init` first", self.state_path.display()))?;
        Ok(serde_json::from_str(&s)?)
    }

    /// Write-then-rename: readers on the read-only bind mount never see a torn file.
    pub fn save_state(&self, state: &State) -> Result<()> {
        let tmp = self.state_path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
        fs::rename(&tmp, &self.state_path)?;
        Ok(())
    }

    pub fn read_journal(&self) -> Result<Vec<Entry>> {
        if !self.journal_path.exists() {
            return Ok(vec![]);
        }
        let text = fs::read_to_string(&self.journal_path)?;
        // The journal is its own WAL: a crash mid-append leaves at most one
        // torn final line, which we skip rather than refuse to load.
        Ok(text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
    }

    pub fn append(&self, e: &Entry) -> Result<()> {
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&self.journal_path)?;
        serde_json::to_writer(&mut f, e)?;
        f.write_all(b"\n")?;
        f.flush()?;
        Ok(())
    }

    /// (next line number, next deploy id). Deploy ids count only entries that
    /// changed routing, so from a clean state they are deterministic: D-1, D-2, ...
    pub fn next_ids(&self) -> Result<(u64, String)> {
        let j = self.read_journal()?;
        let n = j.len() as u64 + 1;
        let d = j.iter().filter(|e| e.deploy_id.is_some()).count() as u64 + 1;
        Ok((n, format!("D-{d}")))
    }
}

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn check_known(service: &str, version: &str) -> Result<()> {
    match KNOWN_VERSIONS.iter().find(|(s, _)| *s == service) {
        None => bail!(
            "unknown service '{service}'; known: {}",
            KNOWN_VERSIONS.iter().map(|(s, _)| *s).collect::<Vec<_>>().join(", ")
        ),
        Some((_, vs)) if !vs.contains(&version) => {
            bail!("unknown version '{version}' for {service}; known: {}", vs.join(", "))
        }
        _ => Ok(()),
    }
}

fn entry(n: u64, kind: &str, service: &str, actor: &str) -> Entry {
    Entry {
        n,
        kind: kind.into(),
        deploy_id: None,
        service: service.into(),
        version: None,
        from_version: None,
        ts: now(),
        actor: actor.into(),
        request_id: None,
        justification_eids: vec![],
        note: None,
    }
}

/// Every service at v1. With `reset`, rotate the journal and start clean.
/// Returns None if state already existed and `reset` was false.
pub fn init(store: &Store, reset: bool) -> Result<Option<Entry>> {
    if store.state_path.exists() && !reset {
        return Ok(None);
    }
    if reset && store.journal_path.exists() {
        let rotated = store
            .journal_path
            .with_file_name(format!("journal-{}.jsonl", Utc::now().format("%Y%m%dT%H%M%SZ")));
        fs::rename(&store.journal_path, &rotated)?;
    }
    let ts = now();
    let state: State = KNOWN_VERSIONS
        .iter()
        .map(|(s, _)| (s.to_string(), ServiceState { version: "v1".into(), deploy_id: None, since: ts.clone() }))
        .collect();
    store.save_state(&state)?;
    let (n, _) = store.next_ids()?;
    let mut e = entry(n, "init", "*", "operator");
    e.version = Some("v1".into());
    e.note = Some("all services at v1".into());
    store.append(&e)?;
    Ok(Some(e))
}

/// Route `service` to `version`. Scenario setup only; never exposed to the agent.
pub fn deploy(store: &Store, service: &str, version: &str, actor: &str) -> Result<Entry> {
    check_known(service, version)?;
    let mut state = store.load_state()?;
    let cur = state.get(service).cloned().context("service missing from state")?;
    let (n, deploy_id) = store.next_ids()?;
    let mut e = entry(n, "deploy", service, actor);
    e.deploy_id = Some(deploy_id.clone());
    e.version = Some(version.into());
    e.from_version = Some(cur.version);
    state.insert(service.into(), ServiceState { version: version.into(), deploy_id: Some(deploy_id), since: e.ts.clone() });
    store.save_state(&state)?;
    store.append(&e)?;
    Ok(e)
}

/// The one mutating action the agent may propose.
///
/// Idempotent on `request_id`; refuses on an `expected_current` mismatch.
/// Every path -- executed, no-op, aborted -- lands in the journal, so a
/// double-fire, a stale proposal, and a real rollback are all auditable.
pub fn rollback(
    store: &Store,
    service: &str,
    to_version: &str,
    request_id: Uuid,
    expected_current: Option<&str>,
    actor: &str,
    justification_eids: Vec<String>,
) -> Result<(Entry, RollbackOutcome)> {
    let journal = store.read_journal()?;
    let n = journal.len() as u64 + 1;

    // Idempotency: a request_id we already acted on is a recorded no-op,
    // never a second rollback. Double-fire is the expected failure mode of a
    // retrying agent, and it must be harmless.
    if let Some(orig) = journal.iter().find(|e| e.request_id == Some(request_id) && e.kind == "rollback") {
        let mut e = entry(n, "noop", service, actor);
        e.version = Some(to_version.into());
        e.request_id = Some(request_id);
        e.justification_eids = justification_eids;
        e.note = Some(format!(
            "duplicate request_id; original entry n={} deploy_id={}",
            orig.n,
            orig.deploy_id.clone().unwrap_or_default()
        ));
        store.append(&e)?;
        return Ok((e, RollbackOutcome::Noop));
    }

    check_known(service, to_version)?;
    let mut state = store.load_state()?;
    let cur = state.get(service).cloned().context("service missing from state")?;

    // TOCTOU: the proposal named the version it was made against. If the
    // world moved between approval and execution, refuse and make the agent
    // re-propose against reality.
    if let Some(exp) = expected_current {
        if exp != cur.version {
            let mut e = entry(n, "aborted", service, actor);
            e.version = Some(to_version.into());
            e.from_version = Some(cur.version.clone());
            e.request_id = Some(request_id);
            e.justification_eids = justification_eids;
            e.note = Some(format!(
                "version mismatch: proposal expected current={exp}, actual current={}",
                cur.version
            ));
            store.append(&e)?;
            return Ok((e, RollbackOutcome::Aborted));
        }
    }

    if cur.version == to_version {
        let mut e = entry(n, "noop", service, actor);
        e.version = Some(to_version.into());
        e.from_version = Some(cur.version);
        e.request_id = Some(request_id);
        e.justification_eids = justification_eids;
        e.note = Some("already at requested version".into());
        store.append(&e)?;
        return Ok((e, RollbackOutcome::Noop));
    }

    let (n, deploy_id) = store.next_ids()?;
    let mut e = entry(n, "rollback", service, actor);
    e.deploy_id = Some(deploy_id.clone());
    e.version = Some(to_version.into());
    e.from_version = Some(cur.version);
    e.request_id = Some(request_id);
    e.justification_eids = justification_eids;
    state.insert(service.into(), ServiceState { version: to_version.into(), deploy_id: Some(deploy_id), since: e.ts.clone() });
    store.save_state(&state)?;
    store.append(&e)?;
    Ok((e, RollbackOutcome::Executed))
}
