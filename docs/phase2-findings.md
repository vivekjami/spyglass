# Phase 2 — Naive baseline: build record

**Objective (spec):** the control group, end to end. Without it every later
number is uninterpretable (ADR-012).
**Built:** 2026-08-29 morning IST · **PR:** #3
**Acceptance bar (spec):** baseline completes an S1 investigation (any outcome)
with metrics captured; the run is screen-recorded for demo segment 2.

---

## Status summary

| Spec output / task | Status | Where |
|---|---|---|
| `bench/conditions/baseline.json` | ✅ | `bench/conditions/` |
| raw-tool MCP server: `tail_logs`, `grep_logs`, `get_metric`, `list_services`, `deploy_events` | ✅ Rust, `rmcp`, :8793 | `crates/rawtools-mcp/` |
| the same gated `rollback` | ✅ `deployer serve`, :8792, `require_approval_for_tools: ["rollback"]` | `deployer/src/main.rs` |
| baseline SOP-lite prompt | ✅ | `agent/baseline-sop.md` |
| run baseline on S1; capture tokens / calls / time | ✅ **completed**: 39.5 s, 19 calls, 198k in-tokens, correct RCA, recovery verified (F5) | `scripts/investigate.py`, `bench/results/` |
| **screen-recorded** | ⏳ **the operator's camera** — steps in `docs/demo.md` §0:10–0:30 | — |
| fairness checklist (same model, info, action path; only shaping differs) | ✅ | `bench/conditions/README.md` |

Deferred per spec: repeats and scoring automation (Phase 10).

---

## Findings and decisions

### F1. The write plane is one library behind two doors

The CLI (`deployer rollback …`) and the MCP tool (`deployer serve` →
`rollback`) call the same `deployer::rollback()` — the function whose
idempotency and TOCTOU behaviour Phase 1 exercised. The MCP server is a
*separate process* from anything that reads telemetry, and `deploy` is not
exposed on it at all: the agent has exactly one way to change the world, and
the agent manifest puts a human on it.

`rollback` returns an `outcome` — `executed | noop | aborted` — as data, not as
an MCP error, so the agent reads a refusal the same way it reads a success and
can re-propose against reality.

### F2. Raw tools are honest tools, not crippled ones

The baseline's tools are shaped like a terminal: `tail -n`, `grep`, `curl
/metrics`, `cat journal`. Three fairness properties, enforced in code:

- **Nothing is hidden.** Any line the engine can read, `grep_logs` can return.
- **Caps exist because real tools have them**, and they are generous: 1000
  lines per call. Every truncated response begins with the count —
  `# 2 of 11812 matching lines … TRUNCATED; narrow the window or raise limit` —
  so the agent can page rather than be misled.
- **No secret shaping**: oldest → newest, verbatim, no dedup, no ranking, no
  templates, no evidence ids.

Smoke-tested against 13 hours of accumulated logs (145,469 gateway lines):
`tail_logs` 186 ms, `grep_logs` across five files 340 ms, `get_metric` 12 ms.

### F3. The condition file is the whole experiment, in one place

`bench/conditions/baseline.json` is a TrueForge agent manifest with every
ADR-016 flag written out explicitly — `compaction: off`,
`large_tool_response: off`, `preload: true` on both MCP servers,
`iteration_limit: 80`, sub-agents and sandbox on — plus `$MODEL_A` resolved
against what the harness actually serves, and the SOP pulled from
`agent/baseline-sop.md` so it can be reviewed as prose. `just tf-setup`
registers it by name; the watcher's `SPYGLASS_AGENT` and the runner's
`--condition` both address it that way. The Spyglass condition (Phase 3) will
differ from this file in exactly one block: `mcp_servers`.

### F4. `pgrep -f` matches the shell that runs it

`just mcp-up` silently did nothing the first time: its `pgrep -f "deployer
serve"` liveness check matched the recipe's own shell, whose command line
contains those words. Same trap as `pkill -f trueforge` in Phase 0.
`scripts/mcp.sh` now checks the *listening port*, as `trueforge.sh` does.
Recorded because it will bite again in any script that manages processes by
name.

### F5. The instrumented run

`scripts/investigate.py` opens a session on the condition's named agent, posts
the alert, drives the approval gate per policy, and writes one JSON per run
with the metrics the benchmark scores plus the complete event trace. Tokens
are summed across every thread (Phase 0 F11); tool calls are counted from
`model.message` events, which with `preload: true` are the agent's own calls
and not the harness's discovery.



### F5 (measured). The baseline solved S1 — in 39.5 seconds, for 198k tokens

Run `s1-baseline-20260829T021835Z` (`bench/results/`), `google-gemini/gemini-3-6-flash`,
approval policy `allow`, against a fresh S1 injected 5 minutes earlier:

| Metric | Value |
|---|---|
| Outcome | **completed** — correct root cause, correct action, recovery verified |
| Wall time | **39.5 s** |
| Turns | 2 (proposal → approval → execute + verify) |
| Tool calls | **19** — `get_metric` 7, `tail_logs` 4, `grep_logs` 2, `current_versions` 2, `deploy_events` 1, `list_services` 1, `rollback` 1, `get_current_datetime` 1 (harness built-in) |
| Model calls | 11, on 1 thread (no sub-agents spawned) |
| Input tokens | **198,106** (per call: 4.5k → 5.9k → 6.1k → 10.5k → 19.6k → 19.8k → 22.1k → 23.2k → 26.7k → 29.8k → 30.0k) |
| Output tokens | 4,978 |
| Tool bytes returned to context | **57,147** (largest single response 22.4 KB: 50 `tail_logs` lines with stack traces) |
| Rollback | `payments v2 → v1`, `D-3`, `expected_current: v2` honoured, `justification_eids: []` |
| 5xx before → after | 20.6% → **0.0%** |

The RCA it wrote is right in every particular: `D-2` at 02:13:34, `647` 500s
from `payments-v2` cascading to 502s, the `validate_v2` stack trace quoted,
non-USD currencies named, the rollback id and request id recorded, recovery
checked with `current_versions`, `get_metric`, and a post-rollback `grep` for
5xx. Its tool sequence was disciplined: inventory → journal → metrics on all
four services → tail the suspect → grep the good version for contrast → act →
re-check.

A first attempt (`s1-baseline-20260829T021502Z`) ended in `harness_error`
before any model call, because of the schema issue in F8. It is committed
too: every run is, including the ones that never started.

### F6. The spec's foil did not appear — and that is the finding

The spec built the demo around "the naive agent drowns" (0:10–0:30). On S1,
with this model, it did not drown: it got there fast and correctly. The
honest reading, recorded before the treatment exists so it cannot be
reverse-engineered afterwards:

1. **S1 is deploy-shaped and loud.** One deploy, one new stack trace on 20% of
   traffic, one cascade. A competent SRE prompt with `tail` and `grep` finds
   that. The accuracy gap the thesis predicts will have to come from the
   scenarios built to be quiet — S2 (no novel error template), S3 (burst of a
   known template, no deploy), S6 (insufficient evidence) — not from S1.
2. **On S1 the comparison is about cost, not correctness.** 198k input tokens
   and 57 KB of raw log paged into context, for an answer whose evidence fits
   in a few hundred bytes. That *is* the thesis's token/cost prediction, and
   it is now a measured number the treatment has to beat.
3. **The demo narration must change.** Not "it failed" — "it got there, at
   200,000 tokens; watch the context grow." The footage shows raw log walls
   and the counter, and the freeze-frame is the totals line. Faking a failure
   would be worse than the truth and easier to catch.
4. **n=1 with a paid-tier model on the easiest scenario.** No generalisation
   is claimed; that is what Phase 10's repeats and S2/S3 are for.

The spec's "wrong" for this phase was *baseline accidentally too weak*. The
opposite happened, which is the better problem to have: the control group is
credible.

### F7. LLM-generated request ids are not random ⚠️ (Phase 9 item)

The rollback's `request_id` — meant to be "a fresh UUID you generate" — was
`9b8c2d1e-3f4a-5b6c-7d8e-9f0a1b2c3d4e`: ascending hex nibbles, a pattern the
model produces on demand, not entropy. Idempotency keyed on a value the model
"invents" fails in the direction that matters: a *later* incident's rollback
with the same pretty id would be journaled as a duplicate no-op and silently
not happen.

Fix (Phase 9 hardening): the system issues the key. Either the deployer
mints a `proposal_id` in a read-only `propose_rollback` step that the gated
`rollback` then consumes, or the runner/harness injects a real UUID. The
model should never be the source of an idempotency token. Added to
`docs/progress.md` open items.

### F8. Gemini rejects schemars's `Option<Vec<T>>`

`justification_eids: Option<Vec<String>>` becomes `anyOf: [array, null]`;
Gemini's function-declaration validator wants `items` at the top level and
fails the *entire* request — every tool, before the first model call. A
`Vec<T>` with `#[serde(default)]` is the shape an optional list must take.
`Option<String>` and `Option<usize>` are accepted. Checked every tool schema
on both servers afterwards: no `anyOf` with an array branch remains.


---

## Reproducing this

```bash
source scripts/env.sh
just build && just mcp-up && just tf-setup     # MCP servers on :8792/:8793; agent 'spyglass-baseline'
S1_FAST=1 just scenario s1                     # fresh incident
just investigate baseline --approval allow     # unattended; --approval ask for the filmed run
```

---

## Spec revisions this phase forces

None. One clarification: the spec's Phase 2 output list names the raw tools
without `current_versions`; it is on the deployer server for both conditions
(it is routing state, not telemetry) and is listed in the fairness mapping.
