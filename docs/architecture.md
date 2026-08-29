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
| C1 | Telemetry ingestion (tailer, normalizer, scraper, backpressure) | Phase 3 | built: tailers + scraper; backpressure = poll cadence, no spill file yet |
| C2 | Evidence store and index (NDJSON segments, template/text/metric indexes) | Phase 3 | built: in-memory store + template index + metric rings; segments written, not yet read |
| C3 | Novelty detection (Drain-style mining, novelty scoring) | Phase 4 | built: level-aware Drain, first-seen/burst scoring with history guards, `novel_templates`; 12 unit tests |
| C4 | Changepoint detection (guarded rolling z-score, deploy correlation) | Phase 5 | built: log-derived request series (error_rate, errors_total, requests_total, latency_ms_mean per service/route/instance), z-score with σ floors and a 30 s guard, `at` refined to the first anomalous event, precision-aware deploy join, `detect_changepoints`; 9 unit tests |
| C5 | Evidence ranking (hand-weighted linear model) | Phase 6 | not started |
| C6 | Evidence bundle generation (bounds, coverage, relationships) | Phase 7 | not started |
| C7 | MCP server (`rmcp`, streamable HTTP) | Phase 3 | built: 8 read tools, eids + digests + latency on every response |
| C8 | Agent SOP (lead prompt + analyst instructions) | Phase 3 | SOP v3 (Phase 5): four-call triage (`freshness_watermark → novel_templates → detect_changepoints → deploy_events`) → hypotheses with eids → contradiction check (incl. `nearest_deploy.relation`) → correlational labelling with the deploy offset → three exits → verify → cited postmortem |
| C9 | Causal verification (exemplar replay) | Phase 8 | sandbox reach **failed** (Phase 0, F9) → replay-as-MCP-tool planned |
| C10 | Human approval gate | Phase 9 | live: `rollback` MCP tool gated via `require_approval_for_tools` (Phase 2); idempotency + TOCTOU in the deployer lib (Phase 1) |
| C11 | Post-action verification loop | Phase 9 | crude version in SOP v1: sandbox sleep → watermark → `error_delta` before/after, 2 checks |

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

## Target system (Phase 1 — built)

The incident environment the engine observes and the agent investigates.
Scenery, not the show — but the evidence it emits is the raw material for
everything upstream, so its shape is specified, not improvised.

```
loadgen ──▶ gateway ──▶ orders ──▶ payments-v1  (known good)
  seed 42    :8080       :8081  ╲▶ payments-v2  (S1 regression)   ← both ALWAYS on
  10 req/s     │           │ ╲
               │           │  ╲▶ postgres (orders table)
               │           ╰──▶ /deploy/current.json  ← written by the deployer
               ╰── request capture (sanitized headers, capped body)
```

| Piece | What it does | Where |
|---|---|---|
| `common` | JSON log formatter, Prometheus metrics, `x-request-id` propagation, deploy-state lookup, deterministic noise | `target-system/common/` |
| `gateway` | public edge; captures each request for later replay (auth headers never captured) | `target-system/gateway/` |
| `orders` | persists to postgres, then charges via whichever payments version `current.json` names — read per request, so a deploy or rollback is a file write, not a restart | `target-system/orders/` |
| `payments` | one codebase, two always-on instances; `v2` carries S1's seeded `UnsupportedCurrency` regression plus two benign novel INFO templates as decoys | `target-system/payments/` |
| `loadgen` | deterministic mixed traffic: 20% non-USD (the S1 failure class), 2% malformed, 1% injection-styled user-agents, seeded WARN chatter | `target-system/loadgen/` |
| `deployer` | Rust CLI: `init`, `deploy`, `rollback`, `current`, `journal`. Atomic state writes; append-only journal that is its own WAL; rollback is idempotent on `request_id` and aborts on a TOCTOU version mismatch | `deployer/` |
| `deployer serve` | the **mutating MCP server** (:8792): `rollback` — approval-required in every agent manifest — and read-only `current_versions`. Same tested lib the CLI uses; `deploy` deliberately absent | `deployer/src/main.rs` |
| `rawtools-mcp` | the **baseline's MCP server** (:8793): `list_services`, `tail_logs`, `grep_logs`, `get_metric`, `deploy_events` — raw lines, generous caps, truncation reported, no shaping | `crates/rawtools-mcp/` |
| `watch` | error-rate dashboard + threshold alert (ADR-013's terminal dashboard); on alert, opens a TrueForge session with the alert as its first turn — `SPYGLASS_AGENT` selects which agent answers | `scripts/watch.py` |
| scenarios | pre-registered ground truth (`SCHEMA.md`), injector, noise profile, reproducibility check | `scenarios/` |

**Evidence contract** (what the engine ingests, README C1):

- Logs: `data/logs/<instance>.jsonl`, one JSON object per line — `ts`,
  `service`, `instance`, `version`, `level`, `req_id`, `msg`, plus `route`,
  `status`, `latency_ms`, `deploy_id`, `upstream`, `stack`, `kind`,
  `headers`/`body` (request capture) when known.
- Metrics: Prometheus text on each service's `/metrics` — `requests_total`,
  `errors_total`, `latency_ms_bucket`, `upstream_requests_total`.
- Deploy events: `data/deploy/journal.jsonl` —
  `{n, kind, deploy_id, service, version, from_version, ts, actor, request_id?, justification_eids?, note?}`.

Acceptance evidence for the S1 scenario lives in
[`scenarios/s1-payment-regression/README.md`](../scenarios/s1-payment-regression/README.md).

## Evidence engine (Phase 3 — built, ugly-but-complete)

```
data/logs/*.jsonl ──tail──▶ normalize ──▶ Store { events, templates(masked), deploys, metric rings, watermarks }
data/deploy/journal ─tail─▶                        │
/metrics ×4 ──scrape 2s──▶                          ▼
                                      tools: search_logs · error_delta · deploy_events
                                             freshness_watermark · get_evidence · service_topology
                                                    │  every response: {result, meta}
                                                    ▼  meta = eids · query_hash · result_digest · window · watermark · lag_ms · engine_latency_ms · bounds
                                      Investigation (= MCP session): E1..En counter, evidence records, ledger/<session>.jsonl
```

| Crate | Holds | Notes |
|---|---|---|
| `spyglass-core` | `Config` (from `spyglass.toml`), `Event`, `DeployEvent`, `Window`, masking → `template_id`, canonical digests (eids stripped), `LedgerEntry`, `Meta`, item byte-capping | no I/O |
| `spyglass-engine` | `Store` + ingest (log/journal tailers on threads, async metrics scraper) + the tools + `Investigation` (eid counter, evidence store, ledger writer) | the spec's ingest/index/detect/rank crates live here as modules until they earn a split |
| `spyglass-mcp` | the read-only `rmcp` server (:8791): stamps eids, computes digests, writes the ledger, attaches `meta` | there is no mutating tool here and never will be |

**Shape after Phase 4:** templates are masked (numbers, ids, hex,
timestamps, currency codes → `<*>`) then routed through a level-aware Drain
tree that owns identity; `novel_templates` scores first-seen and burst novelty
against a history-clipped baseline. `search_logs`
scores by IDF-weighted term fraction plus a phrase bonus — explainable, not
clever; metrics are ingested and watermarked but no tool reads them yet; the
store is rebuilt from the source log files on start (≈10 s for 400k lines)
and segment files are written as a durable copy but not yet read back. A file
that shrinks (the stack was reset) clears the store and bumps `epoch`, so
evidence from a previous incident cannot leak into this one.

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
