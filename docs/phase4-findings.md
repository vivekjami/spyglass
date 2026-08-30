# Phase 4 — Novelty detection: build record

**Objective (spec):** `novel_templates` returns the seeded fault's signature at
rank 1. The headline evidence tool.
**Built:** 2026-08-29, 09:50–11:30 IST · **PR:** #5
**Acceptance bar (spec):** on S1 the seeded template is top-ranked with correct
`first_seen`; on quiet baseline traffic, no high-scoring novelty.

---

## Status summary

| Spec task | Status | Where |
|---|---|---|
| Drain-style miner: masking → tree → similarity merge | ✅ level-keyed, fixed depth 3, threshold 0.5, ≥2 agreeing positions; 7 unit tests | `crates/spyglass-engine/src/drain.rs` |
| first-seen / burst scoring | ✅ pure function, 7 unit tests; weights in `spyglass.toml [novelty]` | `tools.rs::novelty_score` |
| wire tool | ✅ `novel_templates(window, baseline, min_score, limit, services, level)` | `spyglass-mcp` |
| **Acceptance: seeded template rank 1, correct `first_seen`** | ✅ #1, `first_seen` 03:32:43.721 vs `D-2` at 03:32:43.6 (F3) | |
| **Acceptance: quiet traffic → no high-scoring novelty** | ✅ after two guards the first run lacked (F2); three quiet windows return nothing ≥ 0.2 | |
| SOP v2 with `novel_templates` as the headline | ✅ 544 words (was ~700) | `agent/sop.md` |
| Ablation A1 condition (`novel_templates` disabled) | ✅ pulled forward from Phase 10 — it is one `disable_tools` line | `bench/conditions/ablation-no-novelty.json` |

Deferred per spec: cross-service interaction novelty.

---

## Findings and decisions

### F1. Drain, with two rules this corpus taught it

Simplified Drain as specified: mask → tokenise → tree (token count, then
leading tokens, variable-looking tokens routed to `<*>`) → best cluster at the
leaf by fraction of agreeing positions, merge at ≥ 0.5 by wildcarding the
positions that differ. Cluster ids are stable across merges; the pattern is
read from the cluster.

The first real run over S1's logs merged `request completed` (INFO, every
service) and `request failed` (ERROR) into `request <*>` — one agreeing token
of two is exactly the threshold. Two rules, each defensible without S1 in
mind:

1. **The log level is a routing key** (`insert_keyed(level, tokens)`), ahead of
   token count. An ERROR and an INFO line can never meet at a leaf. The level
   is *not* a token, so it does not count as an agreeing position — the first
   attempt made it one, and `INFO request completed` / `INFO request captured`
   promptly merged on "INFO request".
2. **A merge needs at least two agreeing positions** (or a one-token
   message). Two-token messages sharing a word are a coincidence, not a
   template.

Result on S1's logs: 16 templates (masking alone gave 22 because it split
nothing and Drain now merges nothing it should not). Drain's merge fires in
the unit tests (`user alice logged in` / `user bob logged in`), not on S1,
whose variables are all maskable. Said plainly so the algorithm is not
credited for the corpus.

### F2. The quiet-window check caught a false positive that would have shipped

First run: rank 1 correct — and in the *quiet* window before the fault every
steady template scored 0.84–1.0 as a "burst". The baseline window fell before
the engine's history began, so `count_baseline` was 0 for everything, floored
to one event, and steady traffic looked like a 64× jump. A caveat flag had
already named the condition; the scores shipped anyway.

Two guards, both in the pure scoring function and both unit-tested:

- **Warm-up**: a template first seen within `warmup_secs` (30) of the
  engine's earliest event is pre-existing vocabulary. The engine started
  mid-stream; it cannot call that "new".
- **Effective baseline**: the baseline is clipped to real history; with
  fewer than `min_baseline_secs` (60) of it, burst novelty is *undetermined*
  — score 0, reason `insufficient_baseline`, caveat in the payload — never
  inflated. First-seen novelty still applies.

After: the pre-fault quiet window reports *"only 45 s of real baseline history
before the window (need 60); burst novelty is undetermined"* and returns
nothing; the post-rollback and default windows return nothing; the incident
window returns exactly the five templates that first appeared in it.

### F3. Acceptance, measured

Incident window `03:32:30–03:34:40` (`D-2` at 03:32:43.6), effective baseline
96 s, 11 templates in window, engine 17 ms:

| # | novelty | reason | level | stack | count | first seen | instances | pattern |
|---|---|---|---|---|---|---|---|---|
| 1 | 1.000 | first_seen_in_window | **ERROR** | **yes** | 197 | **03:32:43.721** | payments-v2 | `payment validation failed: unsupported currency <*> req=<*>` |
| 2 | 1.000 | first_seen_in_window | ERROR | no | 394 | 03:32:43.725 | gateway, orders | `request failed` |
| 3 | 1.000 | first_seen_in_window | ERROR | no | 197 | 03:32:43.725 | orders | `payments charge failed with HTTP <*>` |
| 4 | 1.000 | first_seen_in_window | ERROR | no | 197 | 03:32:43.729 | gateway | `checkout failed: orders returned HTTP <*>` |
| 5 | 1.000 | first_seen_in_window | INFO | no | 803 | 03:32:43.206 | payments-v2 | `fast-path validation passed for currency <*>` |

Everything else in the window scored 0 (rates steady against the baseline).
Rank 1 is the seeded template, 0.1 s after the deploy, with the stack trace;
the cascade follows in timestamp order; the decoy — new in the same deploy,
harmless, and *first* by time — sits at #5 because it is INFO. That is the
documented sort (novelty, severity, stack, first_seen, count), not a tuned
weight; separating "new and harmful" from "new and harmless" beyond severity
is the ranker's job (Phase 6).

Quiet windows (pre-fault 60 s, post-rollback 4 min, default last 5 min): **0
templates ≥ 0.2**. False-positive check: PASS.

### F4. The agent run

`just demo` with SOP v2 (`s1-spyglass-20260829T041208Z`), the same fault, the
same approval policy as the Phase 3 acceptance:

| Metric | Phase 4 (novelty + SOP v2) | Phase 3 (search-based) | Baseline |
|---|---|---|---|
| Outcome | completed: correct RCA, `D-3` citing `E3, E4`, **20.6% → 0.0%** | completed | completed |
| Tool calls | **12** (`novel_templates` 1, `deploy_events` 1, `get_evidence` 1, `current_versions` 1, `rollback` 1, verification: `exec` 2, `freshness_watermark` 3, `error_delta` 2) | 15 | 19 |
| Model calls | **9** | 16 | 11 |
| Input tokens | **141,601** | 228,628 | 198,106 |
| Output tokens | 7,551 | 6,185 | 4,978 |
| Peak context | 21.5k | 22.8k | 30.0k |
| Tool bytes → context | 31,363 | 32,136 | 57,147 |
| Wall | 70.5 s (incl. two sandbox sleeps) | 63.5 s | 39.5 s |
| Evidence ids cited | **14 of 14** | 15 of 18 | none exist |
| Root cause labelled | correlational, with rejected hypotheses | correlational | unlabelled |
| Ledger re-check | **PASS 4/4** | PASS 7/7 | n/a |

The call sequence is the SOP verbatim: `freshness_watermark → novel_templates
→ deploy_events → get_evidence → current_versions → rollback`, then two
verification rounds. Triage went from five calls to three; `search_logs` was
not needed at all. Input tokens fell **38% against Phase 3** and, for the first
time, **28% below the baseline** — on the scenario where Phase 2 found the
baseline competent. Per-call context: 6.0k → 21.5k over nine calls, against the
baseline's 4.5k → 30.0k over eleven.

What did it: one call that returns the answer-shaped thing (the new template,
with its level, stack flag, first-seen time and instance) instead of two
searches and a dereference; and a shorter SOP that says "three calls, then
decide". The verification protocol is still half the model calls and most of
the tokens after the proposal; Phase 7's bundle and a verification budget are
the remaining levers, and n is still 1.


### F5. Things that pushed back

- The `history_start` line in my own smoke script printed the minimum
  *watermark* instead of the earliest event — a display bug that briefly
  suggested history began after the incident. The payload's `history_start`
  was right; the script was not. Trust the payload.
- Restarting the engine while an agent holds an MCP session would orphan
  the session; the Drain fix landed during the demo's scenario-reset stage,
  checked by grepping the demo log for the agent's start line first.

---

## Reproducing this

```bash
just build && just mcp-up && just tf-setup     # engine now serves novel_templates; ablation agent registered
cargo test --release -p spyglass-engine        # 14 tests: Drain + novelty scoring
DEMO_APPROVAL=allow just demo                  # Spyglass with SOP v2
```
Direct check (any MCP client): `novel_templates` with `window` around a fault
→ the seeded template at #1; with a quiet window → nothing.

---

## Spec revisions this phase forces

1. **C3's score formula** gains the two guards (warm-up, effective baseline)
   and the `insufficient_baseline` outcome. The README's formula block should
   say so.
2. **Drain** is level-keyed and needs two agreeing positions to merge —
   additions to "simplified Drain", recorded in ADR-006.
3. **Ablation A1** exists as a condition file now (it is one line); Phase 10
   only has to run it.
