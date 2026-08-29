# Safety model

**Status:** scaffold — filled in by Phase 9. The design is specified in the root
[`README.md`](../README.md); this file records what was *built and tested*.

Safety here is architectural, not aspirational: each property is enforced by
code, config, or the harness — never by asking the model nicely.

## Read/write separation

- **Read** — every Spyglass engine tool. Side-effect-free against the world.
- **Write** — exactly one tool, `rollback`, on a separate MCP server, marked
  approval-required, idempotent, TOCTOU-checked, journaled, and followed by a
  verification loop.

Adding any future mutation requires the same pattern: its own tool, own gate,
own idempotency key, own verification. That rule is recorded here so scope creep
has to argue with a document.

## Telemetry is DATA, not INSTRUCTIONS

Logs are attacker-writable text that flows into the model's context. Layered
defences:

1. **Structural** — prefer derived facts (template IDs, counts, deltas) over raw
   text; raw excerpts capped, deduped to one per template, and delivered inside
   a JSON field the SOP designates as untrusted.
2. **Bounding** — engine-enforced item and byte limits mean an attacker cannot
   flood the context.
3. **Terminal** — even a fully injected agent cannot mutate anything without a
   human approving a typed, evidence-cited proposal.
4. **Demonstrated** — S1's noise profile writes injection-styled log lines, so
   the defence is exercised rather than claimed. Outcome:
   `[MEASURE AFTER IMPLEMENTATION]`.

## Approval gate — as implemented

Verified in Phase 0 (see [`phase0-findings.md`](phase0-findings.md) F4):

```json
"mcp_servers": [{"name": "spyglass-deployer",
                 "require_approval_for_tools": ["rollback"]}]
```

The harness emits `tool.approval_required` and resumes on `user.tool_approval`
with `{"status":"allow"}` or `{"status":"deny","reason":"..."}`. Deny reasons
are surfaced to the agent.

**As built (Phase 2):** `deployer serve` is a separate MCP server (:8792) from
anything that reads telemetry. It exposes `rollback(service, to_version,
request_id, expected_current?, justification_eids?)` and read-only
`current_versions`; `deploy` is deliberately not there. Every condition's agent
manifest lists it with `require_approval_for_tools: ["rollback"]`, and the CLI
and the MCP tool call the same library function, so the idempotency and TOCTOU
behaviour exercised in Phase 1 is the behaviour behind the gate.

**Verification loop as built (Phase 3, crude):** the SOP requires, after any
action, a sandbox `sleep`, a `freshness_watermark` check, then `error_delta`
(before vs. after) twice; both checks are cited in the postmortem. Measured
in the Phase 3 acceptance: rollback at 03:34, two clean checks, 20.8% → 0.0%,
all five verification calls in the ledger. Gate timeout behaviour, escalation
paths, and a verification budget: **pending** (Phase 9).
