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
| P6 ranking | Sat 12–14 | Sat 13:50–15:00 (run files 09:20–09:29 UTC) | Linear scorer, symmetric factors, cascade dedupe (16 candidates → 6 facts), kind-diverse head: top 3 = template / changepoint / `D-2`. By score alone the INFO decoy beats the deploy; `w_n = 0` moves every score by 0.30 and no position on S1 (all candidates first-seen) — recorded | [phase6-findings.md](phase6-findings.md) · PR #7 |
| P7 bundles | Sat 12–14 | Sat 13:50–15:00 | `build_evidence_bundle`: **8,747 events → 6 items / 5.6 kB** (1458:1), three key facts + relationships by ref, ≤ 8 kB enforced on the payload, compact items with full records behind the eids; SOP v4 opens with it: two-call triage, **11 tool calls, 177.7k / 184.3k input tokens (7–10 % below the baseline), 11/11 eids, re-check PASS** after the safe-watermark fix a one-event late arrival forced | [phase7-findings.md](phase7-findings.md) · PR #7 |
| P8 causal replay | Sat 14–18 | Sat 15:05–15:55 | Executor = the engine (ADR-010 amended): `get_exemplar_request` (sanitized twice, chain + 5xx origin, deterministic) → `replay_exemplar` (N per version, tagged traffic dropped at ingest). **v1 0/20 vs v2 20/20 `separated`** in 1 s, three of three; a succeeding request 0/20 vs 0/20 `not_separated`; 100/100 replay lines excluded. Two agent runs with SOP v5: **CAUSAL** RCA with the one-class limit stated, rollback citing the replay eids, 13/14 and 15/15 eids cited, re-check PASS; 12 calls, 243k / 260k input tokens (of which 162k cache reads — the two extra model calls), uncached +2 % / +24 % vs P7. Baseline gets `http_request`; analyst briefs written, fan-out conditional, not triggered on S1 | [phase8-findings.md](phase8-findings.md) · PR #8 |
| P9 approval + remediation hardened | Sat 18–22 | Sat 15:55–17:05 | **The system mints the key**: `propose_rollback` → gated `rollback(proposal_id, restated)`; expiry (the harness gate has none), TOCTOU at execution, restatement checked, every refusal journaled with its reason; eids rendered at the gate resolved to their ledger lines. **The engine judges recovery**: `verify_recovery` closes on two consecutive clean checks ≥ 15 s apart (`verified_recovery` entry) or escalates (`escalation`, terminal); engine budget backstop. Live: double-fire = 1 rollback + 1 noop; manual change → aborted; expired → aborted; restated mismatch → aborted; closure and escalation both observed; 61st call refused. Agent runs: CAUSAL RCA, engine-closed, 12–13/12–13 eids cited; the deny path ends report-only with no retry | [phase9-findings.md](phase9-findings.md) · PR #9 |
| P10 benchmark | Sun 09–13 | — | | |
| P11 demo + submission | Sun 13–22 | — | | |

**Schedule position after P9:** Sat 17:05 IST vs the spec's Sat 22:00 —
about five hours ahead. ~29 h to the internal deadline. Next: the
benchmark (P10: scenarios S2 and S3, the runner, `report.py`, ablation A1;
S6 above S4/S5), then demo and submission (P11).

## Open decisions

| Decision | Needed by | Options | Recommendation |
|---|---|---|---|
| ~~Idempotency key source (P2 F7)~~ | ~~P9~~ | decided at P9: **(a) built** — `propose_rollback` mints it; `rollback` consumes it | [ADR-011](adr/ADR-011-human-approval-for-destructive-actions.md) |
| ~~Causal-replay executor (P0 F9)~~ | ~~P8~~ | decided at P8: **(A) built** — `replay_exemplar` on the engine; (C) survives as the SOP's "replay not possible" path | [ADR-010](adr/ADR-010-sandbox-verification-before-action.md) |
| Token metric vs RCA label (P8 F6) | P10 | report input tokens alone · report tokens next to the RCA's label (causal / correlational) and the replay outcome | **the latter** — a causal RCA costs two more model calls than a correlational one; comparing tokens without the label would reward the weaker answer |

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
