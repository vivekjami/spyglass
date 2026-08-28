# ADR-002 — Rust for the evidence engine

**Status:** Accepted · **Date:** 2026-08-28

## Context

The engine sits on the hot path of every tool call. Per-call latency is shown on
screen in the demo, so it is part of the argument, not just an implementation
detail. The workload is tail + parse + cluster + index at ingest rate, with
predictable P99s, concurrent with serving bounded queries.

## Decision

Rust, with `tokio` for async and `rmcp` (the official Rust MCP SDK) for the
tool boundary.

## Alternatives considered

- **Python.** Rejected *for this component*: GC pauses and interpreter overhead
  are exactly what hurt at ingest rate, and single-digit-millisecond tool
  latency is part of the demo's claim. (Python is still used for the target
  services — they are scenery, not the show.)
- **Go.** Viable, and rejected only on author leverage: existing Rust indexing
  and anomaly-detection code, and fluency under a 3-day deadline. This is a
  preference backed by schedule risk, not a claim that Go could not do it.

## Consequences

- Agent-side glue and the benchmark runner are TypeScript — a deliberate split
  that also demonstrates range.
- "Why not Go?" is a fair interview question; this record is the answer.
- The `rmcp` ↔ TrueForge interop risk was real enough to be Phase 0 item 2. It
  passed (see [phase0-findings F3](../phase0-findings.md)).

## Reversal conditions

None in scope. A Go port would be a rewrite decision for another day.
