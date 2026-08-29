# Architecture Decision Records

One file per decision: Context → Decision → Alternatives → Why rejected →
Consequences → Reversal conditions.

**Rule:** an ADR is written *when the decision is made*, not backfilled before
submission. A decision recorded after the fact is a rationalisation, and reads
like one.

## Written in full

| ADR | Decision | Why now |
|---|---|---|
| [001](ADR-001-an-evidence-layer-exists.md) | An evidence layer exists | The founding bet; everything else follows from it |
| [002](ADR-002-rust-for-the-engine.md) | Rust for the engine | Exercised in Phase 0 — `rmcp` interop was item 2 |
| [003](ADR-003-mcp-as-the-tool-boundary.md) | MCP as the tool boundary | Exercised in Phase 0; transport question settled by F2 |
| [004](ADR-004-deterministic-retrieval.md) | Deterministic retrieval where possible | Confronted in Phase 3 — ledger digests re-check |
| [005](ADR-005-bounded-evidence-never-unrestricted-telemetry.md) | Bounded evidence, never unrestricted telemetry | Confronted in Phase 3 — engine-side bounds built |
| [006](ADR-006-novelty-via-template-mining.md) | Novelty via template mining, no embeddings | Confronted in Phase 4 — Drain + scoring built, both guards found by the quiet-window check |
| [007](ADR-007-changepoints-via-rolling-zscore.md) | Changepoints via a guarded rolling z-score first | Confronted in Phase 5 — detector built; series source, guard length and `at` precision decided on evidence |
| [008](ADR-008-evidence-ranking-linear-model.md) | Evidence ranking as a hand-weighted linear model | Confronted in Phase 6 — scorer built; what the v0 weights do and do not order on S1, measured |
| [009](ADR-009-evidence-ledger.md) | An evidence ledger, not just an RCA | Confronted in Phase 3 — ledger writer built |
| [010](ADR-010-sandbox-verification-before-action.md) | A controlled replay before any action | Confronted in Phase 8 — built on the engine, not in the sandbox (Phase 0 F9); amended, not reversed |
| [015](ADR-015-scenario-corpus-and-bench-are-durable.md) | Scenario corpus and bench harness are durable artifacts | Confronted in Phase 1 — `scenarios/` built |
| [016](ADR-016-harness-context-management-pinning.md) | Pin harness context-management flags | **New**, forced by a Phase 0 discovery (F7) |
| [017](ADR-017-routing-by-file-versions-always-on.md) | A deploy is a file write; every version always on | **New**, Phase 1 design decision |

## Recorded in the README, expanded when confronted

These decisions are made and are summarised in the root README's ADR section.
Each gets its own file at the phase that actually exercises it — writing them
now would be backfilling in advance.

| ADR | Decision | Expanded at |
|---|---|---|
| 011 | Human approval for destructive actions | Phase 9 |
| 012 | The baseline uses the same model | Phase 2 |
| 013 | No custom frontend initially | Phase 11 |
| 014 | No multi-tenancy, billing, or SaaS infrastructure | — (scope boundary) |
