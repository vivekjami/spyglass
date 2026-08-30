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
//!
//! Phase 9 hardening: the agent no longer supplies the idempotency key. It
//! calls `propose` (non-mutating: records a `proposal` in the journal, mints
//! a `proposal_id`, snapshots the current version and an expiry), then the
//! gated `rollback` *consumes* that proposal by id (`execute`). The agent
//! restates service / version / evidence at the gate so a human reads them
//! there, and the deployer refuses if the restatement differs from what was
//! minted. Expired proposals are refused: a gate the harness never times out
//! still cannot execute a stale approval.

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
    ("orders", &["v1", "v1.1", "v1.2"]), // v1.2: S2's config-only release
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
    /// Proposals: the version the proposal was made against (the TOCTOU witness).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_current: Option<String>,
    /// Proposals: RFC3339 instant after which the proposal can no longer be executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
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
        Self {
            state_path: dir.join("current.json"),
            journal_path: dir.join("journal.jsonl"),
        }
    }

    pub fn load_state(&self) -> Result<State> {
        let s = fs::read_to_string(&self.state_path).with_context(|| {
            format!(
                "read {}; run `deployer init` first",
                self.state_path.display()
            )
        })?;
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
        Ok(text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect())
    }

    pub fn append(&self, e: &Entry) -> Result<()> {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.journal_path)?;
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
            KNOWN_VERSIONS
                .iter()
                .map(|(s, _)| *s)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some((_, vs)) if !vs.contains(&version) => {
            bail!(
                "unknown version '{version}' for {service}; known: {}",
                vs.join(", ")
            )
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
        expected_current: None,
        expires_at: None,
    }
}

/// Every service at v1. With `reset`, rotate the journal and start clean.
/// Returns None if state already existed and `reset` was false.
pub fn init(store: &Store, reset: bool) -> Result<Option<Entry>> {
    if store.state_path.exists() && !reset {
        return Ok(None);
    }
    if reset && store.journal_path.exists() {
        let rotated = store.journal_path.with_file_name(format!(
            "journal-{}.jsonl",
            Utc::now().format("%Y%m%dT%H%M%SZ")
        ));
        fs::rename(&store.journal_path, &rotated)?;
    }
    let ts = now();
    let state: State = KNOWN_VERSIONS
        .iter()
        .map(|(s, _)| {
            (
                s.to_string(),
                ServiceState {
                    version: "v1".into(),
                    deploy_id: None,
                    since: ts.clone(),
                },
            )
        })
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
    let cur = state
        .get(service)
        .cloned()
        .context("service missing from state")?;
    let (n, deploy_id) = store.next_ids()?;
    let mut e = entry(n, "deploy", service, actor);
    e.deploy_id = Some(deploy_id.clone());
    e.version = Some(version.into());
    e.from_version = Some(cur.version);
    state.insert(
        service.into(),
        ServiceState {
            version: version.into(),
            deploy_id: Some(deploy_id),
            since: e.ts.clone(),
        },
    );
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
    if let Some(orig) = journal
        .iter()
        .find(|e| e.request_id == Some(request_id) && e.kind == "rollback")
    {
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
    let cur = state
        .get(service)
        .cloned()
        .context("service missing from state")?;

    // TOCTOU: the proposal named the version it was made against. If the
    // world moved between approval and execution, refuse and make the agent
    // re-propose against reality.
    if let Some(exp) = expected_current
        && exp != cur.version
    {
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
    state.insert(
        service.into(),
        ServiceState {
            version: to_version.into(),
            deploy_id: Some(deploy_id),
            since: e.ts.clone(),
        },
    );
    store.save_state(&state)?;
    store.append(&e)?;
    Ok((e, RollbackOutcome::Executed))
}

// ------------------------------------------------------------------ proposals (Phase 9)

/// Default lifetime of a proposal. The harness's approval gate never expires
/// on its own (Phase 9 finding), so this is the only clock on a pending
/// approval: after it, the rollback is refused and must be re-proposed
/// against the current state.
pub const DEFAULT_PROPOSAL_TTL_SECS: i64 = 600;

fn eid_ok(e: &str) -> bool {
    e.len() > 1 && e.starts_with('E') && e[1..].chars().all(|c| c.is_ascii_digit())
}

/// Record a rollback proposal: non-mutating (no routing change), journaled.
/// Mints the idempotency key the gated `rollback` will consume, snapshots
/// the version the proposal is made against, and stamps an expiry. Refuses
/// proposals that would change nothing or cite no evidence.
pub fn propose(
    store: &Store,
    service: &str,
    to_version: &str,
    justification_eids: Vec<String>,
    actor: &str,
    ttl_secs: i64,
) -> Result<Entry> {
    check_known(service, to_version)?;
    let state = store.load_state()?;
    let cur = state
        .get(service)
        .cloned()
        .context("service missing from state")?;
    if cur.version == to_version {
        bail!("{service} is already at {to_version}; nothing to roll back");
    }
    let mut eids: Vec<String> = justification_eids
        .into_iter()
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect();
    eids.sort();
    eids.dedup();
    if eids.is_empty() {
        bail!("a proposal must cite at least one evidence id (E1, E2, ...)");
    }
    if let Some(bad) = eids.iter().find(|e| !eid_ok(e)) {
        bail!("'{bad}' is not an evidence id (expected E<n>)");
    }
    let (n, _) = store.next_ids()?;
    let mut e = entry(n, "proposal", service, actor);
    e.version = Some(to_version.into());
    e.from_version = Some(cur.version.clone());
    e.expected_current = Some(cur.version);
    e.request_id = Some(Uuid::new_v4());
    e.justification_eids = eids;
    e.expires_at = Some(
        (Utc::now() + chrono::Duration::seconds(ttl_secs.max(1)))
            .to_rfc3339_opts(SecondsFormat::Millis, true),
    );
    e.note = Some(
        "proposal recorded; execution requires human approval of rollback(proposal_id)".into(),
    );
    store.append(&e)?;
    Ok(e)
}

/// What the agent restates at the gate. Every field must agree with the
/// minted proposal -- the human approves what they can read, and the
/// deployer guarantees that is what runs.
#[derive(Debug, Clone, Default)]
pub struct Restated {
    pub service: String,
    pub to_version: String,
    pub expected_current: Option<String>,
    pub justification_eids: Vec<String>,
}

/// Execute an approved proposal. Every refusal is journaled as `aborted`
/// with the reason; a repeat of an executed proposal is a `noop`.
pub fn execute(
    store: &Store,
    proposal_id: Uuid,
    restated: &Restated,
    actor: &str,
) -> Result<(Entry, RollbackOutcome)> {
    let journal = store.read_journal()?;
    let n = journal.len() as u64 + 1;
    let refuse = |n: u64, service: &str, note: String| -> Result<(Entry, RollbackOutcome)> {
        let mut e = entry(n, "aborted", service, actor);
        e.request_id = Some(proposal_id);
        e.version = Some(restated.to_version.clone());
        e.justification_eids = restated.justification_eids.clone();
        e.note = Some(note);
        store.append(&e)?;
        Ok((e, RollbackOutcome::Aborted))
    };

    // Idempotency first: an executed proposal re-sent is a recorded no-op,
    // whatever else has changed since.
    if let Some(orig) = journal
        .iter()
        .find(|e| e.request_id == Some(proposal_id) && e.kind == "rollback")
    {
        let mut e = entry(n, "noop", &orig.service, actor);
        e.version = orig.version.clone();
        e.request_id = Some(proposal_id);
        e.justification_eids = orig.justification_eids.clone();
        e.note = Some(format!(
            "duplicate proposal_id; already executed as entry n={} deploy_id={}",
            orig.n,
            orig.deploy_id.clone().unwrap_or_default()
        ));
        store.append(&e)?;
        return Ok((e, RollbackOutcome::Noop));
    }
    let Some(p) = journal
        .iter()
        .find(|e| e.request_id == Some(proposal_id) && e.kind == "proposal")
    else {
        return refuse(
            n,
            &restated.service,
            format!("unknown proposal_id {proposal_id}; call propose_rollback first"),
        );
    };
    // The restatement must be the proposal.
    let mut want = p.justification_eids.clone();
    want.sort();
    let mut got: Vec<String> = restated
        .justification_eids
        .iter()
        .map(|e| e.trim().to_string())
        .collect();
    got.sort();
    got.dedup();
    let mut diffs = Vec::new();
    if restated.service != p.service {
        diffs.push(format!("service {} != {}", restated.service, p.service));
    }
    if Some(restated.to_version.as_str()) != p.version.as_deref() {
        diffs.push(format!(
            "to_version {} != {}",
            restated.to_version,
            p.version.clone().unwrap_or_default()
        ));
    }
    if let Some(ec) = &restated.expected_current
        && Some(ec.as_str()) != p.expected_current.as_deref()
    {
        diffs.push(format!(
            "expected_current {ec} != {}",
            p.expected_current.clone().unwrap_or_default()
        ));
    }
    if got != want {
        diffs.push(format!("justification_eids {got:?} != {want:?}"));
    }
    if !diffs.is_empty() {
        return refuse(
            n,
            &p.service,
            format!(
                "restated proposal differs from the minted one: {}; re-propose",
                diffs.join("; ")
            ),
        );
    }
    // Expiry.
    if let Some(exp) = p
        .expires_at
        .as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<Utc>>().ok())
        && Utc::now() > exp
    {
        return refuse(
            n,
            &p.service,
            format!(
                "proposal expired at {}; re-propose against the current state",
                p.expires_at.clone().unwrap_or_default()
            ),
        );
    }
    // TOCTOU + execution: the tested path, keyed by the proposal id.
    rollback(
        store,
        &p.service,
        p.version.as_deref().unwrap_or_default(),
        proposal_id,
        p.expected_current.as_deref(),
        actor,
        p.justification_eids.clone(),
    )
}

pub fn find_proposal(store: &Store, proposal_id: Uuid) -> Result<Option<Entry>> {
    Ok(store
        .read_journal()?
        .into_iter()
        .find(|e| e.request_id == Some(proposal_id) && e.kind == "proposal"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("spyglass-deployer-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        init(&store, true).unwrap();
        deploy(&store, "payments", "v2", "deploy-bot").unwrap(); // the fault
        (store, dir)
    }

    fn restated(p: &Entry) -> Restated {
        Restated {
            service: p.service.clone(),
            to_version: p.version.clone().unwrap(),
            expected_current: p.expected_current.clone(),
            justification_eids: p.justification_eids.clone(),
        }
    }

    fn current(store: &Store) -> String {
        store.load_state().unwrap()["payments"].version.clone()
    }

    #[test]
    fn a_proposal_mints_the_key_and_snapshots_the_world() {
        let (store, _d) = fresh();
        let p = propose(
            &store,
            "payments",
            "v1",
            vec!["E3".into(), "E1".into(), "E3".into()],
            "agent",
            600,
        )
        .unwrap();
        assert_eq!(p.kind, "proposal");
        assert!(p.request_id.is_some());
        assert_eq!(p.expected_current.as_deref(), Some("v2"));
        assert_eq!(p.justification_eids, vec!["E1", "E3"]);
        assert!(p.deploy_id.is_none(), "a proposal is not a routing change");
        assert_eq!(current(&store), "v2");
        // ids stay deterministic: the proposal did not consume a D-n
        let (_, next) = store.next_ids().unwrap();
        assert_eq!(next, "D-2");
    }

    #[test]
    fn proposals_that_change_nothing_or_cite_nothing_are_refused() {
        let (store, _d) = fresh();
        assert!(
            propose(&store, "payments", "v2", vec!["E1".into()], "agent", 600)
                .unwrap_err()
                .to_string()
                .contains("already at v2")
        );
        assert!(
            propose(&store, "payments", "v1", vec![], "agent", 600)
                .unwrap_err()
                .to_string()
                .contains("at least one evidence id")
        );
        assert!(
            propose(
                &store,
                "payments",
                "v1",
                vec!["not-an-eid".into()],
                "agent",
                600
            )
            .unwrap_err()
            .to_string()
            .contains("not an evidence id")
        );
    }

    #[test]
    fn double_fire_is_one_rollback_and_one_recorded_noop() {
        let (store, _d) = fresh();
        let p = propose(
            &store,
            "payments",
            "v1",
            vec!["E1".into(), "E7".into(), "E8".into()],
            "agent",
            600,
        )
        .unwrap();
        let id = p.request_id.unwrap();
        let (e1, o1) = execute(&store, id, &restated(&p), "agent").unwrap();
        let (e2, o2) = execute(&store, id, &restated(&p), "agent").unwrap();
        assert_eq!(o1, RollbackOutcome::Executed);
        assert_eq!(e1.deploy_id.as_deref(), Some("D-2"));
        assert_eq!(e1.request_id, Some(id));
        assert_eq!(o2, RollbackOutcome::Noop);
        assert!(e2.note.unwrap().contains("duplicate proposal_id"));
        assert_eq!(current(&store), "v1");
        let kinds: Vec<String> = store
            .read_journal()
            .unwrap()
            .iter()
            .map(|e| e.kind.clone())
            .collect();
        assert_eq!(
            kinds,
            vec!["init", "deploy", "proposal", "rollback", "noop"]
        );
    }

    #[test]
    fn approve_after_manual_rollback_aborts_on_version_mismatch() {
        let (store, _d) = fresh();
        let p = propose(&store, "payments", "v1", vec!["E1".into()], "agent", 600).unwrap();
        // An operator fixes it by hand while the approval is pending.
        deploy(&store, "payments", "v1", "operator").unwrap();
        let (e, o) = execute(&store, p.request_id.unwrap(), &restated(&p), "agent").unwrap();
        assert_eq!(o, RollbackOutcome::Aborted);
        assert!(e.note.unwrap().contains("version mismatch"));
        assert_eq!(current(&store), "v1");
    }

    #[test]
    fn an_expired_proposal_is_never_executed() {
        let (store, _d) = fresh();
        let mut p = propose(&store, "payments", "v1", vec!["E1".into()], "agent", 1).unwrap();
        // Rewrite the expiry into the past rather than sleeping: the check is on the clock, not on a timer.
        let past = (Utc::now() - chrono::Duration::seconds(5))
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let text = fs::read_to_string(&store.journal_path)
            .unwrap()
            .replace(p.expires_at.as_deref().unwrap(), &past);
        fs::write(&store.journal_path, text).unwrap();
        p.expires_at = Some(past);
        let (e, o) = execute(&store, p.request_id.unwrap(), &restated(&p), "agent").unwrap();
        assert_eq!(o, RollbackOutcome::Aborted);
        assert!(e.note.unwrap().contains("expired"));
        assert_eq!(current(&store), "v2", "the world did not move");
    }

    #[test]
    fn a_restatement_that_differs_from_the_proposal_is_refused() {
        let (store, _d) = fresh();
        let p = propose(
            &store,
            "payments",
            "v1",
            vec!["E1".into(), "E2".into()],
            "agent",
            600,
        )
        .unwrap();
        let mut r = restated(&p);
        r.justification_eids = vec!["E9".into()];
        let (e, o) = execute(&store, p.request_id.unwrap(), &r, "agent").unwrap();
        assert_eq!(o, RollbackOutcome::Aborted);
        assert!(e.note.clone().unwrap().contains("justification_eids"));
        let mut r2 = restated(&p);
        r2.to_version = "v2".into();
        assert_eq!(
            execute(&store, p.request_id.unwrap(), &r2, "agent")
                .unwrap()
                .1,
            RollbackOutcome::Aborted
        );
        assert_eq!(current(&store), "v2");
        // the right restatement still works afterwards
        assert_eq!(
            execute(&store, p.request_id.unwrap(), &restated(&p), "agent")
                .unwrap()
                .1,
            RollbackOutcome::Executed
        );
    }

    #[test]
    fn an_unknown_proposal_id_is_refused_and_journaled() {
        let (store, _d) = fresh();
        let r = Restated {
            service: "payments".into(),
            to_version: "v1".into(),
            expected_current: None,
            justification_eids: vec![],
        };
        let (e, o) = execute(&store, Uuid::new_v4(), &r, "agent").unwrap();
        assert_eq!(o, RollbackOutcome::Aborted);
        assert!(e.note.unwrap().contains("unknown proposal_id"));
        assert_eq!(current(&store), "v2");
    }
}
