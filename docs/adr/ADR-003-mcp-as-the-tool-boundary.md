# ADR-003 — MCP as the tool boundary

**Status:** Accepted · **Date:** 2026-08-28

## Context

The agent must call the engine. TrueForge speaks MCP natively. The engine should
outlive this hackathon and this harness.

## Decision

Expose the engine as an **MCP server** using `rmcp`, over **streamable HTTP**.
Tool schemas are derived from Rust types via `schemars`, so the wire contract
and the code cannot drift.

## Alternatives considered

- **Bespoke HTTP + OpenAPI.** Rejected: TrueForge would need an adapter, and the
  engine would be Spyglass-only.
- **Direct library linking.** Rejected: couples process lifecycle and language
  to the harness, and erases the reusability story.
- **stdio transport.** Rejected on design grounds (it ties the engine's lifetime
  to the harness process, whereas the Compose stack should own it). Phase 0 then
  found it is not even available: `MCPServerType` is an enum with the single
  member `"remote"` — see [phase0-findings F2](../phase0-findings.md).

## Consequences

- Typed, schema-validated tool boundaries for free.
- The engine works with any MCP client tomorrow — HolmesGPT, Claude Code, an IDE.
- The engine must be a long-lived HTTP service, which the Compose stack owns.
- Confirmed working end to end in Phase 0: TrueForge enumerated the probe's
  tools with per-field descriptions intact.

## Reversal conditions

If MCP overhead ever dominates tool latency — measure before believing it — add
a fast path. No sign this is needed: probe latency is sub-millisecond.
