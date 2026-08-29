# Build progress

The spec's Build Order, against what actually happened. Updated at the end of
every phase. Times are IST; the hard external deadline is **Mon Aug 31, 00:30
IST** (Sun 20:00 London), internal deadline **Sun Aug 30, 22:00 IST**.

| Phase | Spec slot | Actual | Outcome | Record |
|---|---|---|---|---|
| P0 harness validation | Thu eve | Fri 15:50–17:45 | 6/7 items pass; sandbox reach fails by design, fallback written; free-tier key blocker found and resolved | [phase0-findings.md](phase0-findings.md) · [PR #1](https://github.com/vivekjami/spyglass/pull/1) |
| P1 incident environment | Fri 09–13 | Fri 17:50–19:20 | Acceptance PASS on fast and default timelines; default-timeline runs byte-identical | [phase1-findings.md](phase1-findings.md) · [PR #2](https://github.com/vivekjami/spyglass/pull/2) |
| P2 naive baseline | Fri 13–16 | Sat 07:30–09:00 | Baseline **solved S1** in 39.5 s / 19 calls / 198k in-tokens — correct RCA, verified rollback. The foil did not drown; the comparison on S1 is cost, not correctness. Filming pending (operator) | [phase2-findings.md](phase2-findings.md) · PR #3 |
| P3 minimal Spyglass loop (ugly-but-complete) | Fri 16–23 | Sat 08:00–09:45 | **Loop closed**: `just demo` → cited RCA → gated rollback → verified recovery → ledger re-check 7/7 PASS. Tokens not improved vs baseline (228k vs 198k) — recorded as the number to beat | [phase3-findings.md](phase3-findings.md) · PR #4 |
| P4 novelty | Sat 09–12 | Sat 09:50–11:30 | `novel_templates` rank 1 with `first_seen` +0.1 s; quiet windows clean after two guards the FP check found; agent run **141.6k in-tokens, 12 calls** — 28% below baseline, 38% below P3; 14/14 eids cited | [phase4-findings.md](phase4-findings.md) · PR #5 |
| P5 changepoints | Sat 09–12 | — | | |
| P6 ranking | Sat 12–14 | — | | |
| P7 bundles | Sat 12–14 | — | | |
| P8 causal replay | Sat 14–18 | — | executor decision open (P0 F9) | |
| P9 approval + remediation hardened | Sat 18–22 | — | deployer-side idempotency + TOCTOU already built in P1 | |
| P10 benchmark | Sun 09–13 | — | | |
| P11 demo + submission | Sun 13–22 | — | | |

**Schedule position after P4:** Sat 11:30 IST vs the spec's Sat 12:00 — back
on the calendar for the first time. ~34.5 h to the internal deadline. Next:
changepoints (P5), ranking + bundles (P6/P7), the causal check (P8),
hardening (P9), benchmark (P10).

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
