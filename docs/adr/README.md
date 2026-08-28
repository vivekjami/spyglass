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
| [015](ADR-015-scenario-corpus-and-bench-are-durable.md) | Scenario corpus and bench harness are durable artifacts | Confronted in Phase 1 — `scenarios/` built |
| [016](ADR-016-harness-context-management-pinning.md) | Pin harness context-management flags | **New**, forced by a Phase 0 discovery (F7) |
| [017](ADR-017-routing-by-file-versions-always-on.md) | A deploy is a file write; every version always on | **New**, Phase 1 design decision |

## Recorded in the README, expanded when confronted

These decisions are made and are summarised in the root README's ADR section.
Each gets its own file at the phase that actually exercises it — writing them
now would be backfilling in advance.

| ADR | Decision | Expanded at |
|---|---|---|
| 004 | Deterministic retrieval where possible | Phase 3 (engine v1 + ledger digests) |
| 005 | Bounded evidence, never unrestricted telemetry | Phase 3 |
| 006 | Novelty via template mining, no embeddings | Phase 4 |
| 007 | Changepoint detection via rolling z-score first | Phase 5 |
| 008 | Evidence ranking as a hand-weighted linear model | Phase 6 |
| 009 | An evidence ledger, not just an RCA | Phase 3 |
| 010 | Sandbox verification before action | Phase 8 |
| 011 | Human approval for destructive actions | Phase 9 |
| 012 | The baseline uses the same model | Phase 2 |
| 013 | No custom frontend initially | Phase 11 |
| 014 | No multi-tenancy, billing, or SaaS infrastructure | — (scope boundary) |
