//! deployer CLI, plus `serve`: the mutating MCP server.
//!
//! `serve` exposes exactly one mutating tool, `rollback`, and one read-only
//! helper, `current_versions`. It is a *separate* MCP server from the evidence
//! engine on purpose (README, Safety Model): the read plane and the write
//! plane never share a process, and the agent manifest marks `rollback`
//! approval-required. `deploy` is deliberately absent here.

use std::{path::PathBuf, sync::Mutex};

use anyhow::Result;
use clap::{Parser, Subcommand};
use deployer::{RollbackOutcome, Store};
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
    /// Print current routing state (all services, or one).
    Current { service: Option<String> },
    /// Print the journal.
    Journal,
    /// Run the mutating MCP server (streamable HTTP at /mcp). Tools: rollback, current_versions.
    Serve {
        #[arg(long, default_value_t = 8792)]
        port: u16,
    },
}

// ------------------------------------------------------------------ MCP

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RollbackArgs {
    /// Service to roll back (gateway | orders | payments).
    pub service: String,
    /// Version to roll back to, e.g. "v1".
    pub to_version: String,
    /// A fresh UUID you generate for this proposal. Re-sending the same id is a recorded no-op, never a second rollback.
    pub request_id: String,
    /// The version you observed as current when you formed this proposal. If it no longer matches, the rollback is refused.
    pub expected_current: Option<String>,
    /// Evidence ids (E1, E2, ...) that justify this action. Pass [] if none.
    /// (A plain array, not Option<Vec>: Gemini's function-declaration
    /// validator rejects the anyOf[array, null] schema schemars emits for an
    /// optional list -- "items: missing field". Learned in Phase 2.)
    #[serde(default)]
    pub justification_eids: Vec<String>,
}

#[derive(Clone)]
pub struct DeployerMcp {
    store: std::sync::Arc<Mutex<Store>>,
    tool_router: ToolRouter<DeployerMcp>,
}

#[tool_router]
impl DeployerMcp {
    fn new(store: Store) -> Self {
        Self { store: std::sync::Arc::new(Mutex::new(store)), tool_router: Self::tool_router() }
    }

    /// The ONE mutating action. Approval-required in the agent manifest.
    #[tool(description = "Roll a service back to a version. MUTATING; requires human approval. Idempotent on request_id; refused if expected_current no longer matches the live version. Returns the journal entry and an outcome: executed | noop | aborted.")]
    fn rollback(&self, Parameters(a): Parameters<RollbackArgs>) -> Result<CallToolResult, McpError> {
        let rid = Uuid::parse_str(&a.request_id)
            .map_err(|e| McpError::invalid_params(format!("request_id must be a UUID: {e}"), None))?;
        let store = self.store.lock().map_err(|_| McpError::internal_error("store lock poisoned", None))?;
        let (entry, outcome) = deployer::rollback(
            &store,
            &a.service,
            &a.to_version,
            rid,
            a.expected_current.as_deref(),
            "agent",
            a.justification_eids,
        )
        .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let body = serde_json::json!({
            "outcome": outcome,
            "journal_entry": entry,
            "note": match outcome {
                RollbackOutcome::Executed => "routing changed; verify recovery from telemetry before closing the incident",
                RollbackOutcome::Noop => "nothing changed",
                RollbackOutcome::Aborted => "refused: the live version differs from expected_current; re-check and re-propose",
            },
        });
        Ok(CallToolResult::success(vec![ContentBlock::text(body.to_string())]))
    }

    #[tool(description = "Read-only: which version each service is currently routed to, and the deploy id that put it there.")]
    fn current_versions(&self) -> Result<CallToolResult, McpError> {
        let store = self.store.lock().map_err(|_| McpError::internal_error("store lock poisoned", None))?;
        let state = store.load_state().map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(serde_json::to_string(&state).unwrap_or_default())]))
    }
}

#[tool_handler]
impl ServerHandler for DeployerMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                "Spyglass deployer. `rollback` is the only mutating action and requires human approval; \
                 pass a fresh UUID request_id and the current version you observed as expected_current. \
                 `current_versions` is read-only."
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
        Cmd::Deploy { service, version, actor } => emit(&deployer::deploy(&store, &service, &version, &actor)?),
        Cmd::Rollback { service, to_version, request_id, expected_current, actor, justification_eids } => {
            let (e, outcome) = deployer::rollback(
                &store, &service, &to_version, request_id, expected_current.as_deref(), &actor, justification_eids,
            )?;
            emit(&e)?;
            if outcome == RollbackOutcome::Aborted {
                std::process::exit(2);
            }
            Ok(())
        }
        Cmd::Current { service } => {
            let state = store.load_state()?;
            match service {
                Some(s) => println!("{}", serde_json::to_string(state.get(&s).ok_or_else(|| anyhow::anyhow!("unknown service"))?)?),
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
        Cmd::Serve { port } => serve(store, port),
    }
}

#[tokio::main]
async fn serve(store: Store, port: u16) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let ct = tokio_util::sync::CancellationToken::new();
    let mcp = DeployerMcp::new(store);
    let service = StreamableHttpService::new(
        move || Ok(mcp.clone()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token()),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("deployer MCP server (rollback, current_versions) on http://{addr}/mcp");
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            ct.cancel();
        })
        .await?;
    Ok(())
}
