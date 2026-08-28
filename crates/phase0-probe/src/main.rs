//! Phase 0 probe: the smallest possible Spyglass-shaped MCP server.
//!
//! This is validation scaffolding, not the engine. It answers two questions
//! that every later phase depends on, while it is still cheap to learn the
//! answer is "no":
//!
//!   item 2 — can TrueForge reach a Rust `rmcp` streamable-HTTP server and
//!            invoke its tools? (the Rust <-> TypeScript MCP seam)
//!   item 5 — does `require_approval_for_tools` genuinely block a mutating
//!            tool until a human (or the benchmark runner) approves?
//!
//! Both tools mimic the real engine's response contract so the shape is
//! validated too: every result is JSON, and carries `engine_latency_ms` —
//! the number the demo puts on screen to make the Rust argument empirical.

use std::time::Instant;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};

const BIND_ADDRESS: &str = "127.0.0.1:8791";

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EchoArgs {
    /// Text to echo back in the probe payload.
    pub message: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PretendRollbackArgs {
    /// Service to pretend to roll back.
    pub service: String,
    /// Version to pretend to roll back to.
    pub to_version: String,
}

#[derive(Clone)]
pub struct Probe {
    tool_router: ToolRouter<Probe>,
}

#[tool_router]
impl Probe {
    pub fn new() -> Self {
        Self { tool_router: Self::tool_router() }
    }

    /// Read-only tool. Stands in for the real engine's evidence tools: bounded
    /// JSON out, self-reported latency, no side effects.
    #[tool(description = "Health probe. Returns a bounded JSON payload with engine latency.")]
    fn probe_ping(
        &self,
        Parameters(EchoArgs { message }): Parameters<EchoArgs>,
    ) -> Result<CallToolResult, McpError> {
        let t0 = Instant::now();
        let payload = serde_json::json!({
            "ok": true,
            "echo": message,
            "source": "phase0-probe (rust/rmcp)",
            "engine_latency_ms": t0.elapsed().as_secs_f64() * 1000.0,
        });
        Ok(CallToolResult::success(vec![ContentBlock::text(payload.to_string())]))
    }

    /// Mutating tool. Mutates nothing — it exists solely so we can mark it
    /// approval-required and observe whether the harness actually stops here.
    /// If this returns without an approval round trip, the safety model in
    /// ADR-011 does not hold and the whole plan needs rethinking.
    #[tool(description = "SIMULATED mutating action. Changes nothing; used to test the approval gate.")]
    fn probe_rollback(
        &self,
        Parameters(PretendRollbackArgs { service, to_version }): Parameters<PretendRollbackArgs>,
    ) -> Result<CallToolResult, McpError> {
        let t0 = Instant::now();
        let payload = serde_json::json!({
            "executed": true,
            "simulated": true,
            "service": service,
            "to_version": to_version,
            "note": "no world state was changed; this is a Phase 0 gate probe",
            "engine_latency_ms": t0.elapsed().as_secs_f64() * 1000.0,
        });
        Ok(CallToolResult::success(vec![ContentBlock::text(payload.to_string())]))
    }
}

#[tool_handler]
impl ServerHandler for Probe {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                "Spyglass Phase 0 probe. `probe_ping` is read-only. `probe_rollback` simulates a \
                 mutating action and is used to verify the human approval gate."
                    .to_string(),
            )
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let ct = tokio_util::sync::CancellationToken::new();
    let service = StreamableHttpService::new(
        || Ok(Probe::new()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token()),
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(BIND_ADDRESS).await?;
    tracing::info!("phase0-probe MCP server on http://{BIND_ADDRESS}/mcp");

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            ct.cancel();
        })
        .await?;
    Ok(())
}
