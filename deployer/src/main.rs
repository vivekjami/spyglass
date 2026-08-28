//! Spyglass deployer: the control plane for the target system.
//!
//! This is the *write plane*. The evidence engine never touches it. It owns
//! two files under `--data-dir` (bind-mounted read-only into the services):
//!
//!   current.json   which version each service is routed to (orders reads it
//!                  per request, so a switch takes effect with no restart)
//!   journal.jsonl  append-only record of every deploy / rollback / no-op --
//!                  the highest-prior evidence class, tailed by the engine
//!
//! `deploy` is scenario tooling and is never exposed to the agent. `rollback`
//! is the one mutating action the agent may propose: idempotent on
//! `--request-id`, and it refuses to act if the world moved since the proposal
//! (`--expected-current`). Both live here from Phase 1 because they are twenty
//! lines each and the Phase 3 MCP server should wrap tested behaviour rather
//! than grow its own.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::{SecondsFormat, Utc};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Every version that exists as a runnable artifact. A deploy to anything
/// else is a typo, not a deployment.
const KNOWN_VERSIONS: &[(&str, &[&str])] = &[
    ("gateway", &["v1"]),
    ("orders", &["v1", "v1.1"]),
    ("payments", &["v1", "v2"]),
];

#[derive(Parser)]
#[command(name = "deployer", version, about)]
struct Cli {
    /// Directory holding current.json and journal.jsonl
    #[arg(long, global = true, default_value = "data/deploy")]
    data_dir: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create state (every service at v1) and a journal. --reset rotates the journal and starts clean.
    Init {
        #[arg(long)]
        reset: bool,
    },
    /// Route a service to a version. Scenario setup only; never exposed to the agent.
    Deploy {
        service: String,
        version: String,
        #[arg(long, default_value = "operator")]
        actor: String,
    },
    /// Roll a service back. Idempotent on --request-id; aborts if --expected-current no longer holds.
    Rollback {
        service: String,
        to_version: String,
        #[arg(long)]
        request_id: Uuid,
        /// The version the proposal was made against. Mismatch => abort (TOCTOU check).
        #[arg(long)]
        expected_current: Option<String>,
        #[arg(long, default_value = "agent")]
        actor: String,
        /// Evidence ids justifying the action (E1, E2, ...). Recorded, not validated here.
        #[arg(long = "eid")]
        justification_eids: Vec<String>,
    },
    /// Print current routing state (all services, or one).
    Current { service: Option<String> },
    /// Print the journal.
    Journal,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ServiceState {
    version: String,
    deploy_id: Option<String>,
    since: String,
}

type State = BTreeMap<String, ServiceState>;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Entry {
    n: u64,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    deploy_id: Option<String>,
    service: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    from_version: Option<String>,
    ts: String,
    actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    justification_eids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

struct Store {
    state_path: PathBuf,
    journal_path: PathBuf,
}

impl Store {
    fn new(dir: &Path) -> Self {
        Self { state_path: dir.join("current.json"), journal_path: dir.join("journal.jsonl") }
    }

    fn load_state(&self) -> Result<State> {
        let s = fs::read_to_string(&self.state_path)
            .with_context(|| format!("read {}; run `deployer init` first", self.state_path.display()))?;
        Ok(serde_json::from_str(&s)?)
    }

    /// Write-then-rename: readers on the read-only bind mount never see a torn file.
    fn save_state(&self, state: &State) -> Result<()> {
        let tmp = self.state_path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
        fs::rename(&tmp, &self.state_path)?;
        Ok(())
    }

    fn read_journal(&self) -> Result<Vec<Entry>> {
        if !self.journal_path.exists() {
            return Ok(vec![]);
        }
        let text = fs::read_to_string(&self.journal_path)?;
        // The journal is its own WAL: a crash mid-append leaves at most one
        // torn final line, which we skip rather than refuse to load.
        Ok(text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
    }

    fn append(&self, e: &Entry) -> Result<()> {
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&self.journal_path)?;
        serde_json::to_writer(&mut f, e)?;
        f.write_all(b"\n")?;
        f.flush()?;
        Ok(())
    }

    /// (next line number, next deploy id). Deploy ids count only entries that
    /// changed routing, so from a clean state they are deterministic: D-1, D-2, ...
    fn next_ids(&self) -> Result<(u64, String)> {
        let j = self.read_journal()?;
        let n = j.len() as u64 + 1;
        let d = j.iter().filter(|e| e.deploy_id.is_some()).count() as u64 + 1;
        Ok((n, format!("D-{d}")))
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn check_known(service: &str, version: &str) -> Result<()> {
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

fn emit(e: &Entry) -> Result<()> {
    println!("{}", serde_json::to_string(e)?);
    Ok(())
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    fs::create_dir_all(&cli.data_dir)?;
    let store = Store::new(&cli.data_dir);

    match cli.cmd {
        Cmd::Init { reset } => {
            if store.state_path.exists() && !reset {
                println!("{}", serde_json::to_string_pretty(&store.load_state()?)?);
                return Ok(());
            }
            if reset && store.journal_path.exists() {
                let rotated = cli
                    .data_dir
                    .join(format!("journal-{}.jsonl", Utc::now().format("%Y%m%dT%H%M%SZ")));
                fs::rename(&store.journal_path, &rotated)?;
            }
            let ts = now();
            let state: State = KNOWN_VERSIONS
                .iter()
                .map(|(s, _)| {
                    (s.to_string(), ServiceState { version: "v1".into(), deploy_id: None, since: ts.clone() })
                })
                .collect();
            store.save_state(&state)?;
            let (n, _) = store.next_ids()?;
            let mut e = entry(n, "init", "*", "operator");
            e.version = Some("v1".into());
            e.note = Some("all services at v1".into());
            store.append(&e)?;
            emit(&e)
        }

        Cmd::Deploy { service, version, actor } => {
            check_known(&service, &version)?;
            let mut state = store.load_state()?;
            let cur = state.get(&service).cloned().context("service missing from state")?;
            let (n, deploy_id) = store.next_ids()?;
            let mut e = entry(n, "deploy", &service, &actor);
            e.deploy_id = Some(deploy_id.clone());
            e.version = Some(version.clone());
            e.from_version = Some(cur.version);
            state.insert(service, ServiceState { version, deploy_id: Some(deploy_id), since: e.ts.clone() });
            store.save_state(&state)?;
            store.append(&e)?;
            emit(&e)
        }

        Cmd::Rollback { service, to_version, request_id, expected_current, actor, justification_eids } => {
            let journal = store.read_journal()?;
            let n = journal.len() as u64 + 1;

            // Idempotency: a request_id we already acted on is a recorded
            // no-op, never a second rollback. Double-fire is the expected
            // failure mode of a retrying agent, and it must be harmless.
            if let Some(orig) = journal.iter().find(|e| e.request_id == Some(request_id) && e.kind == "rollback") {
                let mut e = entry(n, "noop", &service, &actor);
                e.version = Some(to_version);
                e.request_id = Some(request_id);
                e.justification_eids = justification_eids;
                e.note = Some(format!(
                    "duplicate request_id; original entry n={} deploy_id={}",
                    orig.n,
                    orig.deploy_id.clone().unwrap_or_default()
                ));
                store.append(&e)?;
                return emit(&e);
            }

            check_known(&service, &to_version)?;
            let mut state = store.load_state()?;
            let cur = state.get(&service).cloned().context("service missing from state")?;

            // TOCTOU: the proposal named the version it was made against. If
            // the world moved between approval and execution, refuse and
            // make the agent re-propose against reality.
            if let Some(exp) = expected_current.as_deref() {
                if exp != cur.version {
                    let mut e = entry(n, "aborted", &service, &actor);
                    e.version = Some(to_version);
                    e.from_version = Some(cur.version.clone());
                    e.request_id = Some(request_id);
                    e.justification_eids = justification_eids;
                    e.note = Some(format!(
                        "version mismatch: proposal expected current={exp}, actual current={}",
                        cur.version
                    ));
                    store.append(&e)?;
                    emit(&e)?;
                    std::process::exit(2);
                }
            }

            if cur.version == to_version {
                let mut e = entry(n, "noop", &service, &actor);
                e.version = Some(to_version);
                e.from_version = Some(cur.version);
                e.request_id = Some(request_id);
                e.justification_eids = justification_eids;
                e.note = Some("already at requested version".into());
                store.append(&e)?;
                return emit(&e);
            }

            let (n, deploy_id) = store.next_ids()?;
            let mut e = entry(n, "rollback", &service, &actor);
            e.deploy_id = Some(deploy_id.clone());
            e.version = Some(to_version.clone());
            e.from_version = Some(cur.version);
            e.request_id = Some(request_id);
            e.justification_eids = justification_eids;
            state.insert(service, ServiceState { version: to_version, deploy_id: Some(deploy_id), since: e.ts.clone() });
            store.save_state(&state)?;
            store.append(&e)?;
            emit(&e)
        }

        Cmd::Current { service } => {
            let state = store.load_state()?;
            match service {
                Some(s) => println!("{}", serde_json::to_string(state.get(&s).context("unknown service")?)?),
                None => println!("{}", serde_json::to_string_pretty(&state)?),
            }
            Ok(())
        }

        Cmd::Journal => {
            for e in store.read_journal()? {
                emit(&e)?;
            }
            Ok(())
        }
    }
}
