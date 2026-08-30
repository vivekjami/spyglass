//! deployer CLI, plus `serve`: the mutating MCP server.
//!
//! `serve` exposes exactly one mutating tool, `rollback`, plus two
//! non-mutating helpers: `propose_rollback` (records a proposal, mints the
//! idempotency key) and `current_versions`. It is a *separate* MCP server
//! from the evidence engine on purpose (README, Safety Model): the read
//! plane and the write plane never share a process, and the agent manifest
//! marks `rollback` approval-required. `deploy` is deliberately absent here.

use std::{path::PathBuf, sync::Mutex};

use anyhow::Result;
use clap::{Parser, Subcommand};
use deployer::{Restated, RollbackOutcome, Store};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use uuid::Uuid;

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
    /// Record a rollback proposal (mints the proposal_id the gated rollback consumes). Non-mutating.
    Propose {
        service: String,
        to_version: String,
        #[arg(long = "eid")]
        justification_eids: Vec<String>,
        #[arg(long, default_value = "agent")]
        actor: String,
        /// Seconds until the proposal expires.
        #[arg(long, default_value_t = deployer::DEFAULT_PROPOSAL_TTL_SECS)]
        ttl_secs: i64,
    },
    /// Execute a recorded proposal by id (restated from the proposal itself). The operator's path to the same checks the gate applies.
    Execute {
        proposal_id: Uuid,
        #[arg(long, default_value = "operator")]
        actor: String,
    },
    /// Print current routing state (all services, or one).
    Current { service: Option<String> },
    /// Print the journal.
    Journal,
    /// Run the mutating MCP server (streamable HTTP at /mcp). Tools: propose_rollback, rollback, current_versions.
    Serve {
        #[arg(long, default_value_t = 8792)]
        port: u16,
        /// Seconds a proposal stays executable after it is recorded.
        #[arg(long, default_value_t = deployer::DEFAULT_PROPOSAL_TTL_SECS)]
        proposal_ttl_secs: i64,
    },
}

// ------------------------------------------------------------------ MCP

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ProposeArgs {
    /// Service to roll back (gateway | orders | payments).
    pub service: String,
    /// Version to roll back to, e.g. "v1".
    pub to_version: String,
    /// Evidence ids (E1, E2, ...) that justify the action. At least one.
    #[serde(default)]
    pub justification_eids: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RollbackArgs {
    /// The proposal_id from propose_rollback. It is the idempotency key: re-sending it is a recorded no-op, never a second rollback.
    pub proposal_id: String,
    /// Restate the proposal so the approver reads it at the gate: the service.
    pub service: String,
    /// Restated: the version to roll back to.
    pub to_version: String,
    /// Restated: the version the proposal recorded as current. Refused if the live version differs (the world moved).
    pub expected_current: Option<String>,
    /// Restated: the proposal's justification evidence ids. Must match the proposal exactly.
    #[serde(default)]
    pub justification_eids: Vec<String>,
}

#[derive(Clone)]
pub struct DeployerMcp {
    store: std::sync::Arc<Mutex<Store>>,
    ttl_secs: i64,
    #[allow(dead_code)] // read by rmcp's #[tool_router]/#[tool_handler] macros
    tool_router: ToolRouter<DeployerMcp>,
}

fn json_text(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(
        v.to_string(),
    )]))
}

#[tool_router]
impl DeployerMcp {
    fn new(store: Store, ttl_secs: i64) -> Self {
        Self {
            store: std::sync::Arc::new(Mutex::new(store)),
            ttl_secs,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Record a rollback PROPOSAL (no routing change): validates the target, snapshots the current version, mints the proposal_id that the gated `rollback` consumes, stamps an expiry, journals it. Pass service, to_version and the evidence ids that justify it. Returns the proposal to restate at the gate."
    )]
    fn propose_rollback(
        &self,
        Parameters(a): Parameters<ProposeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let store = self
            .store
            .lock()
            .map_err(|_| McpError::internal_error("store lock poisoned", None))?;
        let p = deployer::propose(
            &store,
            &a.service,
            &a.to_version,
            a.justification_eids,
            "agent",
            self.ttl_secs,
        )
        .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        json_text(serde_json::json!({
            "proposal_id": p.request_id,
            "service": p.service, "to_version": p.version, "expected_current": p.expected_current,
            "justification_eids": p.justification_eids, "expires_at": p.expires_at, "journal_entry": p,
            "next": "call rollback(proposal_id, service, to_version, expected_current, justification_eids) restating exactly these values; it requires human approval and is refused if the live version has moved or the proposal has expired",
        }))
    }

    /// The ONE mutating action. Approval-required in the agent manifest.
    #[tool(
        description = "Execute an approved rollback PROPOSAL. MUTATING; requires human approval. Pass the proposal_id from propose_rollback and restate service, to_version, expected_current and justification_eids so the approver reads them here. Idempotent on proposal_id (a repeat is a recorded no-op); refused -- and journaled as aborted -- if the restatement differs from the proposal, the proposal has expired, or the live version is no longer expected_current. Returns the journal entry and an outcome: executed | noop | aborted."
    )]
    fn rollback(
        &self,
        Parameters(a): Parameters<RollbackArgs>,
    ) -> Result<CallToolResult, McpError> {
        let pid = Uuid::parse_str(&a.proposal_id).map_err(|e| {
            McpError::invalid_params(
                format!("proposal_id must be the UUID minted by propose_rollback: {e}"),
                None,
            )
        })?;
        let store = self
            .store
            .lock()
            .map_err(|_| McpError::internal_error("store lock poisoned", None))?;
        let restated = Restated {
            service: a.service,
            to_version: a.to_version,
            expected_current: a.expected_current,
            justification_eids: a.justification_eids,
        };
        // Refusals (unknown proposal, restatement mismatch, expiry, version moved) are
        // journaled `aborted` entries returned as Ok; an Err here is the journal or
        // state file failing, which is the server's fault, not the caller's.
        let (entry, outcome) = deployer::execute(&store, pid, &restated, "agent")
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        json_text(serde_json::json!({
            "outcome": outcome,
            "journal_entry": entry,
            "note": match outcome {
                RollbackOutcome::Executed => "routing changed; verify recovery from telemetry before closing the incident (verify_recovery with this deploy_id)",
                RollbackOutcome::Noop => "nothing changed",
                RollbackOutcome::Aborted => "refused; see journal_entry.note -- re-check the world and re-propose if the action is still warranted",
            },
        }))
    }

    #[tool(
        description = "Read-only: which version each service is currently routed to, and the deploy id that put it there."
    )]
    fn current_versions(&self) -> Result<CallToolResult, McpError> {
        let store = self
            .store
            .lock()
            .map_err(|_| McpError::internal_error("store lock poisoned", None))?;
        let state = store
            .load_state()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let text = serde_json::to_string(&state)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }
}

#[tool_handler]
impl ServerHandler for DeployerMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                "Spyglass deployer. propose_rollback records a proposal and mints its proposal_id (no change); \
                 rollback(proposal_id, ...) is the only mutating action, requires human approval, and is refused if the \
                 proposal expired, the restatement differs, or the live version moved. current_versions is read-only."
                    .to_string(),
            )
    }
}

fn emit(e: &deployer::Entry) -> Result<()> {
    println!("{}", serde_json::to_string(e)?);
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.data_dir)?;
    let store = Store::new(&cli.data_dir);

    match cli.cmd {
        Cmd::Init { reset } => match deployer::init(&store, reset)? {
            Some(e) => emit(&e),
            None => {
                println!("{}", serde_json::to_string_pretty(&store.load_state()?)?);
                Ok(())
            }
        },
        Cmd::Deploy {
            service,
            version,
            actor,
        } => emit(&deployer::deploy(&store, &service, &version, &actor)?),
        Cmd::Rollback {
            service,
            to_version,
            request_id,
            expected_current,
            actor,
            justification_eids,
        } => {
            let (e, outcome) = deployer::rollback(
                &store,
                &service,
                &to_version,
                request_id,
                expected_current.as_deref(),
                &actor,
                justification_eids,
            )?;
            emit(&e)?;
            if outcome == RollbackOutcome::Aborted {
                std::process::exit(2);
            }
            Ok(())
        }
        Cmd::Propose {
            service,
            to_version,
            justification_eids,
            actor,
            ttl_secs,
        } => emit(&deployer::propose(
            &store,
            &service,
            &to_version,
            justification_eids,
            &actor,
            ttl_secs,
        )?),
        Cmd::Execute { proposal_id, actor } => {
            let p = deployer::find_proposal(&store, proposal_id)?
                .ok_or_else(|| anyhow::anyhow!("unknown proposal_id {proposal_id}"))?;
            let restated = Restated {
                service: p.service.clone(),
                to_version: p.version.clone().unwrap_or_default(),
                expected_current: p.expected_current.clone(),
                justification_eids: p.justification_eids.clone(),
            };
            let (e, outcome) = deployer::execute(&store, proposal_id, &restated, &actor)?;
            emit(&e)?;
            if outcome == RollbackOutcome::Aborted {
                std::process::exit(2);
            }
            Ok(())
        }
        Cmd::Current { service } => {
            let state = store.load_state()?;
            match service {
                Some(s) => println!(
                    "{}",
                    serde_json::to_string(
                        state
                            .get(&s)
                            .ok_or_else(|| anyhow::anyhow!("unknown service"))?
                    )?
                ),
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
        Cmd::Serve {
            port,
            proposal_ttl_secs,
        } => serve(store, port, proposal_ttl_secs),
    }
}

#[tokio::main]
async fn serve(store: Store, port: u16, ttl_secs: i64) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let ct = tokio_util::sync::CancellationToken::new();
    let mcp = DeployerMcp::new(store, ttl_secs);
    let service = StreamableHttpService::new(
        move || Ok(mcp.clone()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token()),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(
        "deployer MCP server (propose_rollback, rollback, current_versions; proposal ttl {ttl_secs}s) on http://{addr}/mcp"
    );
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            ct.cancel();
        })
        .await?;
    Ok(())
}
