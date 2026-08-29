# Phase 3 — Minimal Spyglass loop: build record

**Objective (spec):** agent → MCP → engine → evidence → RCA, end to end, ugly.
The highest-risk integration seam, crossed while there is still time to react.
**Built:** 2026-08-29 morning IST · **PR:** #4
**Acceptance bar (spec):** one command runs S1 with the Spyglass agent through
to verified recovery; ledger file exists and digests re-check.

---

## Status summary

| Spec output / task | Status | Where |
|---|---|---|
| engine serving `search_logs`, `error_delta`, `deploy_events`, `freshness_watermark` | ✅ plus `get_evidence`, `service_topology` | `crates/spyglass-engine`, `crates/spyglass-mcp` |
| eids + digests + latency on every response | ✅ `meta` block on every tool result | `spyglass-mcp/src/main.rs::respond` |
| ledger writer | ✅ engine-side, per MCP session: `ledger/<session>.jsonl` + `.evidence.jsonl` | `spyglass-engine/src/lib.rs::Investigation` |
| SOP v1 | ✅ | `agent/sop.md` |
| Phase 9 minimum: gated `rollback` + crude verification | ✅ rollback from Phase 2; verification = sandbox sleep → watermark → `error_delta` before/after ×2 | SOP §7 |
| text-scoring decision (timebox) | ✅ hand-rolled: IDF-weighted term fraction + phrase bonus, grouped by template. No tantivy. | F3 |
| **Acceptance: one command → verified recovery** | ✅ `just demo` exit 0; attempt 4 (F6) | `just demo` |
| **Acceptance: ledger exists, digests re-check** | ✅ attempt 4: 7/7 deterministic entries match (attempt 3's mismatch found and fixed a real hole) | `scripts/ledger-check.py` |

Deferred per spec: novelty (P4), changepoints (P5), ranking (P6), bundles
(P7), subagents and causal replay (P8).

---

## Findings and decisions

### F1. One MCP session is one investigation

The engine has no notion of the harness's sessions, but every MCP client
opens its own MCP session, and `rmcp` exposes the `mcp-session-id` header on
every request. That id keys everything: the `E1…En` counter, the evidence
records `get_evidence` dereferences, and the ledger file. The runner reads the
same id from TrueForge's `mcp.initialize` event, so a result JSON can name the
ledger it came from. No plumbing between harness and engine was needed.

### F2. `meta` is the contract

Every response is `{"result": …, "meta": …}`. `meta` carries: the eids issued
(the ids to cite), `query_hash` and `result_digest` (16 hex chars each, full
values in the ledger), the resolved `window`, the per-source `watermark` map,
`lag_ms`, `engine_latency_ms`, `deterministic`, and `bounds`
(`max_items / items_returned / items_available / truncated`). The agent is
told what it did not see, how stale what it saw is, and how to cite it — in
every response, without asking.

### F3. Text scoring: the timebox chose the simple thing

The spec left "hand-rolled postings vs. tantivy" open for a 3-hour timebox.
It took 20 minutes to decide: search results are *templates*, not documents,
and there are 22 of them in a 200k-event store. An inverted index is the wrong
tool for a two-digit corpus. `search_logs` groups matching events by template
inside the window, scores each template by IDF-weighted fraction of query
terms present (plus 0.5 for the whole phrase), and sorts by score desc, count
desc, first_seen asc, template_id asc. Explainable in one sentence, and
deterministic by construction.

Honest limitation, visible in the smoke test: on the query
`error failed exception`, all four ERROR templates scored 0.154 and `request
failed` ranked first on count. Relevance is not the same as *evidence
weight*; that is what Phase 6's ranker is for. Meanwhile `has_stack` and
`instances` on each item let the agent tell the root (`payments-v2`, stack)
from the cascade.

### F4. Masking is a good-enough template miner for now

Numbers, UUIDs, hex runs, RFC3339 timestamps, and three-letter uppercase
tokens (currency codes) become `<*>`. 196,738 events → 22 templates, and the
seeded error is exactly one of them:
`payment validation failed: unsupported currency <*> req=<*>`. Drain's tree
and similarity merge (Phase 4) exist for logs less regular than these;
recorded so nobody mistakes Phase 3 masking for the novelty detector.

### F5. Evidence, windowed, over the incident hour (smoke test)

Against the store an hour after the Phase 2 incident, with explicit windows:

| Query | Result |
|---|---|
| `error_delta` before `D-2` vs. during | `payments 0.000 → 0.205`, `gateway → 0.201`, `orders → 0.201` (n≈3,000 each) |
| `search_logs` ERROR in the incident window | 4 templates; the seeded one ×670 on `payments-v2`, **`first_seen` 02:13:34.844**, `has_stack: true` — 0.3 s after `D-2` at 02:13:34.542; cascade templates at `.852` (orders) and `.858` (gateway) — **root first, by timestamp** |
| `search_logs "validation"` | the decoy `fast-path validation passed for currency <*>` INFO ×2596 beside the ERROR ×670 — both present, distinguishable by level |
| same query twice | same digest |
| engine latency | 0.05 ms (watermark) to 7.5 ms (search over 200k events) |

The first search, with the *default* window, returned nothing — correctly:
the default is the last 15 minutes of ingested data and the incident was an
hour old. "Not now" is an answer.

### F6. The acceptance run

`just demo` = `mcp-up` → `tf-setup` → fresh S1 (fast timeline) → the Spyglass
agent → gated rollback → verification → ledger re-check. Four attempts, all
kept:

| Attempt | Failed at | Why | Fix |
|---|---|---|---|
| 1 | `tf-setup` | `PUT /agents/{id}` rejects `name` — the `spyglass` agent had never been created | update body is manifest-only |
| 2 | turn 1, before any model call | Gemini rejects `Option<WindowArg>` (`anyOf` + `$ref`/`$defs`) | `WindowArg` inlined, non-optional, default = no window |
| 3 | **completed the loop** — re-check `FAIL` (1 of 7) | `deploy_events` with no window meant "the whole journal", recorded as `window: null`; the agent's own `D-3` landed in the journal seconds later and the replay saw it | every tool resolves and records a window; the demo exits non-zero on a re-check failure |
| 4 | **passed** — re-check 7/7, exit 0 | | |

**Attempt 3 — the loop, end to end** (`s1-spyglass-20260829T032658Z`):

| Metric | Spyglass (attempt 3) | Baseline (Phase 2) |
|---|---|---|
| Outcome | completed: correct RCA, `D-3` rollback citing `E1, E5, E7`, 19.8% → **0.0%** | completed: correct RCA, `D-3`, 20.6% → 0.0% |
| Tool calls | **14** (`freshness_watermark` ×3, `error_delta` ×3, `search_logs` ×2, `deploy_events`, `current_versions`, `get_evidence`, `service_topology`, `rollback`, sandbox `exec` ×1) | 19 |
| Model calls | 15 | 11 |
| Input tokens | **201,297** | 198,106 |
| Output tokens | 8,389 (a 15-citation postmortem) | 4,978 |
| Peak context | **21.4k** | 30.0k |
| Tool bytes → context | **31,020** | 57,147 |
| Wall time | 69.6 s (incl. a 25 s sandbox `sleep` for fresh telemetry) | 39.5 s |
| Evidence ids cited in RCA | **15 of 15 issued** | none exist |
| Root cause labelled | **"CORRELATIONAL … causal replay is not supported in this version"** | unlabelled |
| Verification | two structured `error_delta` checks after a watermark check, both cited | ad hoc metric re-reads |
| Engine latency | p50 0.59 ms, max 10.9 ms | n/a |

Read honestly:

- **The loop works.** Alert → bounded evidence → hypotheses with ids →
  contradiction check (the RCA's timeline puts the first `payments-v2` error
  at 03:25:49.110, 0.5 s *after* `D-2`, and the decoy `D-1` a minute earlier
  with no effect) → a proposal that cites its evidence at the gate → an
  executed rollback → verification the postmortem can point at. That is the
  Phase 3 objective, and the Friday-night milestone, on Saturday morning.
- **Tokens did not improve. Bytes and calls did.** 201k vs 198k input tokens
  is a wash; what moved is peak context (−30%), bytes served to the model
  (−46%), tool calls (−26%), and everything to do with accountability. Why
  tokens did not move: the SOP is 1,038 tokens against the baseline's ~350
  and is re-sent on every call; the verification protocol is five model
  calls the baseline did not make; every response carries a `meta` block;
  and there is no bundle tool yet, so triage is five calls that Phase 7
  collapses into one. The thesis's token prediction is **not supported at
  n=1 on S1 by the Phase 3 engine**, and that is the number to beat in
  Phases 4–7 — recorded now so it cannot be tuned toward afterwards.
- **The postmortem is the ledger rendered.** Every timeline line, every
  claim, every verification check carries an id that dereferences. The
  baseline's RCA was correct and well-evidenced; it was not *checkable*.

**Attempt 4 — acceptance** (`s1-spyglass-20260829T033353Z`), after the
`deploy_events` window fix:

| Metric | Value |
|---|---|
| Outcome | **completed**: correct RCA, `D-3` rollback citing `E1, E2, E3, E5, E7, E8, E9`, **20.8% → 0.0%** |
| Ledger re-check | **PASS — 7 match, 0 mismatch, 5 skipped** (3 temporal `freshness_watermark`, 1 session-scoped `get_evidence`, 1 sandbox `exec` is not a ledger entry) |
| `just demo` exit code | 0 |
| Tool calls | 15 (`freshness_watermark` ×4, `error_delta` ×4, `search_logs` ×2, `deploy_events`, `current_versions`, `get_evidence`, `rollback`, `exec`) |
| Model calls / input / output tokens | 16 / **228,628** / 6,185 |
| Peak context | 22.8k |
| Tool bytes → context | 32,136 |
| Wall | 63.5 s |
| Evidence ids | 18 issued, **15 cited** in the RCA; root cause labelled correlational: yes |
| Engine latency | p50 0.80 ms, max 6.66 ms |

The acceptance bar — *one command runs S1 with the Spyglass agent through to
verified recovery; ledger file exists and digests re-check* — is met. Tokens
went **up** against attempt 3 (228k vs 201k): the agent ran four verification
rounds instead of two. The cost of disciplined verification on a 20k-token
context is real and is now measured; Phase 7's bundle and a tighter
verification budget in the SOP are the levers, and they get measured too.


### F7. Things that pushed back

- **Yesterday's probe still held :8791.** `just mcp-up` reported the engine
  "already up"; TrueForge listed `probe_ping` as its tools. Port-based
  liveness is right but cannot say which process owns the port — the tool
  list can. Killed the probe; the crate is deleted in this PR.
- **`PUT /agents/{id}` rejects `name`.** `tf-setup` had been failing on the
  first existing agent for an hour, silently, so the `spyglass` agent was
  never created until `just demo` tripped on it. Fixed; the demo recipe runs
  `tf-setup` first precisely so this class of drift cannot hide.
- **Gemini rejects `Option<Struct>` too.** schemars emits
  `anyOf: [{$ref: "#/$defs/WindowArg"}, {type: null}]` for an optional
  window; Gemini's validator fails the whole request before the first model
  call (the second time this class of bug cost a run — Phase 2 F8 was
  `Option<Vec>`). The rule now: no `anyOf`, no `$ref`, no `$defs` in any tool
  schema. `WindowArg` is `#[schemars(inline)]` and non-optional with a serde
  default; "no window" is the default value. `tf-setup` could grow a schema
  lint; for now the check is a one-liner in the findings.
- **The runner crashed on its own new code** (`UnboundLocalError`) after the
  first Spyglass turn errored — the ledger block ran before the metrics dict
  existed, so the harness-error run was not written to `bench/results/`. The
  harness log records it; this note is the honest substitute.
- **Startup rebuild is visible.** Querying 4 s after start showed only
  `gateway` in `error_delta` — the tailer was still on its first pass through
  the other files. The per-source `watermark` map is the honest signal
  (missing source = not yet ingested); the SOP's "watermark first" rule
  covers it.

---

## Reproducing this

```bash
source scripts/env.sh
just build && just mcp-up && just tf-setup
DEMO_APPROVAL=allow just demo         # or plain `just demo` to approve by hand
just ledger-check ledger/<investigation>.jsonl
```

---

## Spec revisions this phase forces

1. **Crate layout** — ingest/index live as modules inside `spyglass-engine`
   rather than the spec's seven crates; the README tree says so. Split when a
   module earns it, not before.
2. **Ledger authorship** — the engine writes evidence entries; the action and
   verification entries the spec sketches (`sandbox.replay`,
   `verify.error_delta`) arrive with Phase 8/9's client-side wrapper. The
   deployer journal already records the action with its `justification_eids`.
3. **Segments** — written, not yet read; the store rebuilds from the source
   logs on start. Recorded as the Phase 3 shape, not the design.
