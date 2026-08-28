# Architecture

> Component-level detail. For *why* any of this exists, read
> [`motivation.md`](motivation.md) first. For the system diagram and data flow,
> see the root [`README.md`](../README.md).

**Status:** scaffold. Sections are filled in by the phase that builds them, so
this file never describes code that does not exist.

## The three load-bearing separations

1. **Harness vs. domain.** TrueForge owns the agent loop, subagents, sandbox,
   approvals, and sessions. Spyglass owns evidence and the investigation SOP.
   Nothing is duplicated across the line — rebuilding sponsor infrastructure is
   a liability, not an achievement.
2. **Read plane vs. write plane.** The entire evidence engine is read-only
   against the world. Exactly one mutating tool exists (`rollback`), on a
   *separate* MCP server, behind the harness approval gate (ADR-011).
3. **Evidence vs. explanation.** The engine computes facts — templates, deltas,
   changepoints — deterministically. The model composes explanations. Facts
   carry IDs; explanations cite them (ADR-009).

## Components

| ID | Component | Built in | Status |
|---|---|---|---|
| C1 | Telemetry ingestion (tailer, normalizer, scraper, backpressure) | Phase 3 | not started |
| C2 | Evidence store and index (NDJSON segments, template/text/metric indexes) | Phase 3 | not started |
| C3 | Novelty detection (Drain-style mining, novelty scoring) | Phase 4 | not started |
| C4 | Changepoint detection (guarded rolling z-score, deploy correlation) | Phase 5 | not started |
| C5 | Evidence ranking (hand-weighted linear model) | Phase 6 | not started |
| C6 | Evidence bundle generation (bounds, coverage, relationships) | Phase 7 | not started |
| C7 | MCP server (`rmcp`, streamable HTTP) | Phase 3 | probe validated (Phase 0) |
| C8 | Agent SOP (lead prompt + analyst instructions) | Phase 3 | not started |
| C9 | Causal verification (exemplar replay) | Phase 8 | sandbox reach **failed** (Phase 0, F9) → replay-as-MCP-tool planned |
| C10 | Human approval gate | Phase 9 | mechanism validated (Phase 0) |
| C11 | Post-action verification loop | Phase 9 | not started |

## Harness integration (validated in Phase 0)

Everything here was verified against TrueForge 0.1.4 — see
[`phase0-findings.md`](phase0-findings.md) for evidence and commands.

- **MCP registration** is `remote`-only (HTTP); there is no stdio transport.
  `POST /api/v1/settings/mcp-servers` with `{type, name, url, description}`.
- **Approval gating** is per-agent, per-MCP-server:
  `mcp_servers[].require_approval_for_tools: ["@all"|"@write"|"@destructive"|<tool>]`.
  The harness emits `tool.approval_required` and accepts `user.tool_approval`
  as a turn input item — so approvals work both in the UI and unattended.
- **Tool visibility** is `enable_tools` / `disable_tools` with `@all` and
  `@read-only` selectors, which makes the benchmark's conditions and the
  novelty ablation pure config changes.
- **Sandbox** runs locally via `@anthropic-ai/sandbox-runtime` (needs `bwrap`,
  `socat`, `rg` on the host) — no Daytona cloud account. It is
  **network-isolated by design** and the harness's egress allowlist is
  hard-coded, so it cannot reach the Compose stack (phase0-findings F9). The
  causal replay therefore runs as a bounded MCP tool on the evidence plane.
- **Tool loading**: deferred by default, which adds `list_tools`/`get_tool_info`
  calls per tool used; `preload: true` is pinned in every benchmark condition
  (ADR-016).
- **Budgets**: subagents are dynamic and share the root agent's tools, so
  per-subagent budgets are advisory prompt text. Real limits are
  `config.iteration_limit` and engine-side rate limiting.

## The Kubernetes delta — documented, not built

The demo runs on Docker Compose because it is deterministic on a judge's
machine and the k8s delta adds failure modes without adding thesis value. For
completeness, the delta is:

- **Log ingestion**: a DaemonSet tailer reading container logs per node,
  instead of a Compose volume tail.
- **Metrics**: a ServiceMonitor / Prometheus scrape config, instead of the
  engine scraping endpoints directly.
- **Deploy events**: watch Deployment/ReplicaSet objects and their revision
  history, instead of the deployer's JSONL journal.
- **Rollback**: `kubectl rollout undo` against a Deployment, instead of
  Compose service replacement. The idempotency key, TOCTOU version check, and
  approval gate are unchanged — they are properties of the deployer tool, not
  of the orchestrator.
