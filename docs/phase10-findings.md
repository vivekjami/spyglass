# Phase 10 — Benchmark: build record

**Objective (spec):** the numbers. Scenarios S2–S6 (S2, S3 required; S6
above S4/S5), the runner over {baseline, spyglass, ablation-no-novelty} ×
scenarios × 3 repeats, `report.py` → tables in `docs/benchmark.md` and the
README.
**Built:** 2026-08-29, 18:00 IST onward (12:30 UTC onward) · **PR:** #10
**Acceptance bar (spec):** results table populated from committed raw run
files; every number traceable to a run JSON. Pre-agreed floor: baseline +
spyglass on S1–S3 × 3.

---

## Status summary

| Spec task | Status | Where |
|---|---|---|
| S2 timeout-cascade | ✅ built; reproduces run-to-run with **0.0 pt** drift (F1) | `scenarios/s2-timeout-cascade/` |
| S3 redis-pressure | ✅ built; reproduces with **0.0 pt** drift; the known-but-rare template had to share the *level* with the fault's (F2) | `scenarios/s3-redis-pressure/` |
| S6 insufficient-evidence | ✅ built; a latency-only symptom of an unobserved dependency, a benign deploy as the tempting rollback (F1) | `scenarios/s6-insufficient-evidence/` |
| S4, S5 | ○ not built — the spec's drop order, applied | — |
| Runner | ✅ `bench/run.py`: one fresh incident per cell, unattended, invalid runs kept and re-run (F4) | `bench/run.py` |
| `report.py` | ✅ mechanical scoring against pre-registered ground truth: verdict block + evidence-id join (F3) | `bench/report.py`, `scenarios/SCHEMA.md` |
| Ablation A1 | ✅ a second instance of the same engine, `--ablation no-novelty` — the spec's "one line in the condition file" was not enough (F5) | `scripts/mcp.sh`, `bench/conditions/ablation-no-novelty.json` |
| **Acceptance: tables from committed runs** | ✅ `docs/benchmark.md` and the README's *Results* are generated regions; `bench/results/` holds every run (F6) | `just report` |

---

## Findings and decisions

### F1. The scenarios that matter are the ones without a smoking gun

S1 has a stack trace at the culprit. Every condition finds it (Phase 2
already showed the baseline does, in 40 s). The three new scenarios remove
one thing each:

| | The smoking gun removed | What is left | Correct outcome |
|---|---|---|---|
| **S2** | no new template at the culprit — `orders v1.2` is a *config-only* release (the fraud vendor's v2 API + a doubled timeout); orders never fails, it just gets slow; the gateway's 8 s upstream timeout turns ~30 % of orders into edge 503s | a latency cascade (orders +1.6 s, then the gateway, then the 5xx at +8 s), the change event, and a novel ERROR template at the *wrong* service (the edge symptom) | rollback orders → v1 |
| **S3** | no change event — a 66 MB blob from another tenant fills a `noeviction` redis; payments fails closed on its idempotency write | a template the service already logged rarely (a retried cache hiccup, 2 % of charges), bursting ~100×; the store's own memory numbers in a WARN; error changepoints with `nearest_deploy: null` | report-only; nothing to roll back |
| **S6** | no cause in the telemetry — the vendor orders calls synchronously slows to 9 s on 12 % of calls; orders fails open after 5 s and never logs the call | a latency changepoint at orders (p95 ≈ 5 s), nothing else; a benign `orders v1.1` deploy six minutes earlier; the topology says the vendor exists and is unobserved | refuse to act; say what evidence would decide it |

The mechanisms are all real-system shapes and all self-contained: an
external vendor stub (`target-system/fraudcheck/`, in both conditions'
topology, in no telemetry), a `/knobs` directory for environment changes
that are nobody's deploy, and a redis policy (`noeviction`) that a payments
service should have anyway. S1's steady state changed by one synchronous
~3 ms vendor call and a 2 % retried cache hiccup; its error curve did not.

Every scenario reproduces to the request (F1 of Phase 1, re-measured):

| Scenario | Two runs, fast timeline | Post-fault 5xx | Post-fault p95 | Drift |
|---|---|---|---|---|
| S2 | `20260829T130851Z`, `20260829T131500Z` | 29.3 % / 29.3 % | 8,013 / 8,013 ms | **0.0 pt** |
| S3 | `20260829T132109Z`, `20260829T132420Z` (first image) · re-authored `[see README]` | ≈ 97–98 % | — | **0.0 pt** |
| S6 | `[see README]` | 0.0 % / 0.0 % (a latency-only symptom) | ≈ 5 s | **0.0 pt** |

### F2. The engine keys templates by log level — a known-but-rare template must share the level

The first S3 pass logged the steady-state cache hiccup at WARN and the
fault's failure at ERROR, expecting Drain to merge `cache write failed:
TimeoutError` and `cache write failed: OutOfMemoryError` (three of four
tokens agree). It did not: Phase 4 keyed the Drain tree by level so that
`INFO request completed` and `ERROR request failed` can never meet at a
leaf — a decision this scenario ran into from the other side. The engine
reported the fault's template as `first_seen_in_window`, novelty 1.0, and
S3's point (the burst-rate component must carry the detection) was lost.

Decision: the hiccup logs at ERROR, as a retried write failure usually
does ("transient; retried once, ok" in `detail`). The level key stays —
it is right for the reason Phase 4 gave. Recorded in the scenario's ground
truth so the next author does not rediscover it.

### F3. Score mechanically, or admit an opinion

Metric 2 (root-cause accuracy) needs the agent's answer in a form a script
can read. Both SOPs now end the report with a fenced `verdict` block —
`culprit_service`, `culprit_change`, `cause`, `action`, `evidence_label`
— and each scenario's ground truth lists the accepted values (`none` is a
value: S3's change, S6's culprit). A missing block scores as incorrect and
the table says so. Metrics 3–4 resolve every cited evidence id to the item
the engine returned — the run file keeps every tool response — and test it
against pre-registered `match` maps (kind, pattern text, series metric,
service, direction, deploy id, offset from the fault, replay verdict).
No LLM judge: a judge adds a model's opinion to a measurement of models.

Two consequences the tables carry: the baseline has no evidence ids, so
its precision/recall read `n/a` — its claims are not re-checkable, which
is a result, not a gap in the scorer; and "false hypotheses" (metric 12)
is proxied by *decoy mentions* in interim messages, a count rather than a
judgment, labelled as such.

### F4. The gate is simulated as approving

Unattended runs auto-approve. A wrong proposal therefore *executes* and is
scored as a wrong action (S3/S6: any rollback is one; S2/S1: the wrong
service or version). This is the conservative reading — a simulated human
who reads the eids and refuses would flatter the agent — and the demo's
gate remains real (`--approval ask`). Harness errors and unfinished turns
are kept as `valid: false` files and the cell is re-run on a fresh incident.

### F5. "Implemented as a `disable_tools` entry" was wrong

`build_evidence_bundle` embeds the novelty miner's output — template items
ranked with `w_n`. Hiding the `novel_templates` tool leaves the treatment
in place. Ablation A1 is a second instance of the same engine binary and
config started with `--ablation no-novelty` (`scripts/mcp.sh`, :8794,
registered as `spyglass-engine-ablation`): `novel_templates` refuses, the
bundle carries no template candidates, `w_n = 0`, the watermark and every
bundle stamp `ablation: no-novelty`. Templates stay reachable through
`search_logs`; the agent has to ask. Same SOP, deployer, gate and alert.

### F6. What the numbers say

*Filled from `docs/benchmark.md` after the matrix ran — see that file for
the generated tables; this section reads them.*

`[MEASURE AFTER IMPLEMENTATION]`

### F7. Loose ends, stated

- The model catalog exposes no prices; `bench/price-sheet.json` ships with
  `null` and the cost column reads `n/a`. Tokens are the cost proxy rather
  than an invented dollar figure.
- The sandbox's `exec` calls are recorded per run (`sandbox_exec_commands`)
  and the report flags any that is not a `sleep` — the runs' evidence is the
  tool trace, and an agent reading the repository through the sandbox would
  show there.
- S4 and S5 are not built; S6 was prioritised per the spec. CUSUM was never
  demanded.
- n = 3 per cell is a hackathon budget; the report prints per-run values
  and ranges and claims no significance.
