# Build progress

The spec's Build Order, against what actually happened. Updated at the end of
every phase. Times are IST; the hard external deadline is **Mon Aug 31, 00:30
IST** (Sun 20:00 London), internal deadline **Sun Aug 30, 22:00 IST**.

| Phase | Spec slot | Actual | Outcome | Record |
|---|---|---|---|---|
| P0 harness validation | Thu eve | Fri 15:50–17:45 | 6/7 items pass; sandbox reach fails by design, fallback written; free-tier key blocker found and resolved | [phase0-findings.md](phase0-findings.md) · [PR #1](https://github.com/vivekjami/spyglass/pull/1) |
| P1 incident environment | Fri 09–13 | Fri 17:50–19:20 | Acceptance PASS on fast and default timelines; default-timeline runs byte-identical | [phase1-findings.md](phase1-findings.md) · [PR #2](https://github.com/vivekjami/spyglass/pull/2) |
| P2 naive baseline | Fri 13–16 | Sat 07:30–09:00 | Baseline **solved S1** in 39.5 s / 19 calls / 198k in-tokens — correct RCA, verified rollback. The foil did not drown; the comparison on S1 is cost, not correctness. Filming pending (operator) | [phase2-findings.md](phase2-findings.md) · PR #3 |
| P3 minimal Spyglass loop (ugly-but-complete) | Fri 16–23 | Sat 08:00–09:07 | **Loop closed**: `just demo` → cited RCA → gated rollback → verified recovery → ledger re-check 7/7 PASS. Tokens not improved vs baseline (228k vs 198k) — recorded as the number to beat | [phase3-findings.md](phase3-findings.md) · PR #4 |
| P4 novelty | Sat 09–12 | Sat 09:10–09:45 | `novel_templates` rank 1 with `first_seen` +0.1 s; quiet windows clean after two guards the FP check found; agent run **141.6k in-tokens, 12 calls** — 28% below baseline, 38% below P3; 14/14 eids cited | [phase4-findings.md](phase4-findings.md) · PR #5 |
| P5 changepoints | Sat 09–12 | Sat 09:55–14:00 (incl. a 2.5 h host suspend mid-phase) | `detect_changepoints` localises S1 to **+0.5 s**, `D-2` annotated, cascade in causal order; decoy deploy on nothing; 10.7 idle minutes → 0 changepoints; 23 unit tests; agent run re-checks 5/5 and cites changepoint offsets, but S1 tokens rose to 222.6k (the tool adds evidence, not savings, where novelty already answers — S2 is its case). Series from the logs (deterministic), guard 30 s, precision-aware deploy relation — three spec corrections | [phase5-findings.md](phase5-findings.md) · PR #6 |
| P6 ranking | Sat 12–14 | Sat 14:30–16:30 | Linear scorer, symmetric factors, cascade dedupe (16 candidates → 6 facts), kind-diverse head: top 3 = template / changepoint / `D-2`. By score alone the INFO decoy beats the deploy; `w_n = 0` moves every score by 0.30 and no position on S1 (all candidates first-seen) — recorded | [phase6-findings.md](phase6-findings.md) · PR #7 |
| P7 bundles | Sat 12–14 | Sat 14:30–16:30 | `build_evidence_bundle`: **8,747 events → 6 items / 5.6 kB** (1458:1), three key facts + relationships by ref, ≤ 8 kB enforced on the payload, compact items with full records behind the eids; SOP v4 opens with it: two-call triage, **11 tool calls, 177.7k / 184.3k input tokens (7–10 % below the baseline), 11/11 eids, re-check PASS** after the safe-watermark fix a one-event late arrival forced | [phase7-findings.md](phase7-findings.md) · PR #7 |
| P8 causal replay | Sat 14–18 | — | executor decision open (P0 F9) | |
| P9 approval + remediation hardened | Sat 18–22 | — | deployer-side idempotency + TOCTOU already built in P1 | |
| P10 benchmark | Sun 09–13 | — | | |
| P11 demo + submission | Sun 13–22 | — | | |

**Schedule position after P7:** Sat 16:45 IST vs the spec's Sat 14:00 —
under three hours behind. ~29 h to the internal deadline. Next: the causal
check (P8, executor decision open), hardening (P9), benchmark (P10), demo
and submission (P11).

## Open decisions

| Decision | Needed by | Options | Recommendation |
|---|---|---|---|
| Idempotency key source (P2 F7) | P9 | (a) deployer mints a `proposal_id` in a read-only step the gated `rollback` consumes · (b) runner injects a real UUID | **(a)** — the model must never be the source of an idempotency token |
| Causal-replay executor (P0 F9) | P8 | (A) `replay_exemplar` MCP tool on the evidence plane · (B) Daytona + public tunnel · (C) bisection, correlational RCA | **(A)** — the controlled experiment survives; only its executor changes |

## Drop order if behind (from the spec, unchanged)

k8s notes → dashboards → S4/S5 → CUSUM → Model-B runs → subagents (sequential
analysis, same SOP) → sandbox replay (bisection, claims downgraded) → S6. The
never-drop core: P0–P3, novelty, gated idempotent rollback + verification,
ledger with re-checkable digests, S1–S3 benchmarked ×3, failure-first video,
Qodo trail + README, `just demo` on a clean machine.

## Prerequisites discovered so far (all in the README's Setup section)

Node ≥ 22.14 · `bwrap`/`socat`/`rg` for the local sandbox · `just` · PyYAML ·
a **paid-tier** model key · a free host port for the gateway (8080 is usually
taken).
