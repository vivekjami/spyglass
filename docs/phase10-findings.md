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
| S3 | `20260829T135308Z`, `20260829T140004Z` (300 s steady; two earlier passes, 0.0 pt each, taught F2) | 98.7 % / 98.7 % | — | **0.0 pt** |
| S6 | `20260829T134326Z`, `20260829T134815Z` (130 s lead; an earlier 50 s-lead pass taught F1b) | 0.0 % / 0.0 % (a latency-only symptom) | 5,071 / 5,068 ms | **0.0 pt** |

### F1b. A fast timeline can be a different scenario

S6's first fast pass put the benign `orders v1.1` deploy 50 s before the
symptom, as S1's fast timeline does. In S1 that is harmless: the fault's
own deploy is 0.6 s from the changepoint and wins the nearest-deploy join.
In S6 there is no closer deploy, so the engine annotated the latency
changepoints `changepoint_after_deploy +53.9 s`, `D-1`, and the bundle
drew `D-1 -[precedes_within_120s]->` both — evidence that, by the SOP's
own rules, makes the decoy a legitimate suspect. That is not the
pre-registered scenario ("six minutes earlier, outside the correlation
window"); it is a harder and unfair one. The fast lead is now 130 s, past
the engine's ±120 s join, and the ground truth says so. The general rule:
a compressed timeline must preserve every *relation* the engine computes,
not just the order of events.

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

The second pass found the other half: with the levels aligned the template
merged — and vanished from `novel_templates` entirely. Burst novelty is
`rate_window / rate_baseline`, the baseline is the 15 minutes *before* the
engine's default 5-minute window, and the score is undetermined (0, never
inflated — a Phase 4 guard) with fewer than 60 s of real baseline. A
160-second run sits wholly inside the window; there is no baseline. A
first-seen template does not need one (S1's never did), a bursting one
does. So S3's steady state is 360 s by default and 300 s on the fast
timeline, leaving ≥ 60 s of history before the window; the engine is
unchanged. "Known-but-rare" is a statement about history, and the scenario
has to supply it.

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

One smoke run before the matrix (Spyglass on S3: `report_only`, `redis /
none`, 7 of 7 eids valid) showed three cited items the ground truth had no
class for — the journal's `init` entry cited as "nothing was deployed", the
exemplar, and the replay that failed on *both* versions ("not a property of
a version"). All three are evidence *for* the right answer, so S2, S3 and
S6 gained supporting (`key: false`) entries for them before any benchmark
run; recorded here because "pre-registered" has to mean before the runs it
scores, and this is the only edit the ground truth received after an agent
had seen a scenario.

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

*Read from the generated tables in `docs/benchmark.md` (36 runs, 36 valid,
0 harness errors; the matrix ran unattended 14:08–18:43 UTC on one host);
the per-run table there names the file behind every number. Means over
n = 3 with ranges; no significance is claimed.*

| | S1 payment regression | S2 config-only release | S3 redis full, no change | S6 unobserved vendor |
|---|---|---|---|---|
| **Success** baseline / Spyglass / A1 | 3/3 · 3/3 · 3/3 | 3/3 · 3/3 · 3/3 | 3/3 · 3/3 · 3/3 | **0/3 · 1/3 · 3/3** |
| No wrong action | 3/3 · 3/3 · 3/3 | 3/3 · 3/3 · 3/3 | 3/3 · 3/3 · 3/3 | 2/3 · **1/3** · 3/3 |
| RCA correct | 3/3 · 3/3 · 3/3 | 3/3 · 3/3 · 3/3 | 3/3 · 3/3 · 3/3 | 2/3 · 1/3 · 3/3 |
| Tool calls | 19 · 18 · 21 | 26 · 30 [15..38] · 39 | 14 · **8.7** · 10 | 29 · 27 · 22 |
| Input tokens (uncached) | 424k (115k) · 461k (112k) · 525k (141k) | 891k (213k) · 937k (177k) · 1253k (228k) | 210k (83k) · **139k (50k)** · 157k (65k) | 1328k (232k) · 705k (145k) · 400k (121k) |
| Alert → RCA | 63 s · 78 s · 83 s | 102 s · 109 s · 126 s | 49 s · 46 s · 49 s | 125 s · 99 s · 81 s |
| Evidence recall (Spyglass / A1) | 100 % · 87 % | 100 % · 100 % | 89 % · **56 %** | 100 % · 100 % |

**1. Where the cause is in the telemetry, the same model finds it with raw
tools too.** S1–S3: 9/9 correct actions and RCAs for *every* condition. The
baseline rolled back the config-only release (S2) and reported the redis
fill with nothing to roll back (S3) three times each. The pre-registered
prediction — *more accurate* — is not supported on S1–S3 by this model on
these scenarios; correctness there is a tie. What the evidence plane
changes on S1–S3 is the *shape* of the investigation, and only sometimes
the bill.

**2. The bill: cheaper on S3, not on S1 and S2.** S3 is the clean win —
8.7 calls against 14, 139k input tokens against 210k (−34 %; uncached −40
%), same answer, every claim cited. On S1 Spyglass costs 9 % more input
tokens (uncached −3 %) and 15 s more wall time; on S2, 5 % more (uncached
−17 %) and 7 s. Both are the action path, not the investigation: the
causal check (2 calls) and the engine-judged verification — 15 of the 54
S1 Spyglass calls and 25 of 89 on S2 are `verify_recovery`, most of them
`too_soon` polls (F6a). One S2 run that did not poll finished in 15 calls
and 329k tokens; the two that did took 36–38 calls and 1.2–1.3 M. The
baseline has no verification loop to pay for: it re-reads a metric twice
and declares recovery itself.

**3. What the table cannot show for the baseline: the citations.** Every
Spyglass RCA cites 7–22 evidence ids; each resolves to the engine's
record. Precision as pre-registered (relevant ÷ cited) reads 92 % on S1,
95 % on S3 and 46–47 % on S2 and S6 — the low numbers are the metric's
denominator, not wrong citations: it counts the causal check's exemplar
and replay ids and the verification checks as *other* (the ground truth
has no matcher for them; they are the action's evidence, not the
cause's). Over root-cause citations alone — relevant ÷ (relevant +
decoy), computed post hoc from the same per-eid classification, not a
change to the scorer — Spyglass is at 91–100 % on S1–S3 and cited a decoy
in four runs out of nine; on S6, 70–100 %, three decoy citations in the
run that blamed the postgres chatter. The baseline's claims have no ids
and are not re-checkable — a property, not a score.

**4. What novelty buys, measured by taking it away.** With the templates
gone (A1) recall drops where the smoking gun *is* a template: S3 56 %
(the bursting `cache write failed` template is the evidence; A1 finds the
redis WARN through `search_logs` instead) and S1 87 %; calls and tokens
rise on S1 (+3 calls, +14 %) and S2 (+9 calls, +34 %) as the model
searches for what the bundle no longer hands it. Correctness does not
move on S1–S3: the changepoints and deploy correlation carry the
answer there.

**5. S6 is the negative result, and the ablation is the surprise.** The
scenario has a real symptom (p95 +500 ms at orders and the gateway), no
error, no change event, an unobserved vendor as the cause, and a benign
deploy 130 s earlier. The pre-registered right answer is to refuse and
say what would decide it. **A1 refused 3/3**, each time with the
correlation-window argument in its own words (*"D-1 occurred 129 s prior
with normal operation in between, and telemetry cannot observe the
external fraudcheck dependency"*). **Spyglass refused 1/3** and rolled the
benign deploy back twice; the baseline never refused — it filed
`report_only` naming `fraudcheck / none` twice (the *No wrong action*
column shows that floor) and rolled the deploy back once. The two wrong
Spyglass runs were built on templates: one cited five `search_logs` hits
and called the latency "cascading … following orders deploy D-1"; the
other cited the `postgres insert slower than budget` WARN chatter — the
scenario's decoy, present before and after the fault — as the mechanism.
With novelty off the bundle is a latency changepoint with `nearest_deploy:
null` and a deploy outside the window, and the model applied the SOP's
rule every time. **More evidence made the agent act.** That is the
thesis's failure mode stated in ADR-001 — an evidence plane that hands
the model material for a story is worse than none — and it is the finding
this benchmark exists to be able to produce. The engine and SOP gaps it
exposes are in F6d.

**6. Verification.** S1: the engine closed 3/3 Spyglass and 3/3 A1 runs
(`verified_recovery`); the baseline's 3/3 is its own re-read. S2: 2/3 —
one false escalation (F6b). S6: the two wrong rollbacks were "verified"
by a 5xx-only check (F6d). S3 has no action to verify.

**7. The sandbox never ran a command.** Every `exec` in every run —
36 in the matrix and every run since Phase 3 — returned the same harness
error: *Sandbox initialization failed: Failed to pip install pydantic …
Cannot connect to proxy*. The sandbox is enabled in every manifest and the
agent calls it (the SOP's `sleep 15` between verification checks; the
baseline's `ls -la`, and one attempt to read `data/logs/orders.jsonl`
through the filesystem — refused with the same error, so no condition
read anything the tools did not serve). Symmetric across conditions, so
the comparison stands; but the pacing `sleep` fell back to polling (F6a),
and "sandboxed code execution" was not exercised by any recorded run.
Diagnosed and handled in Phase 11 (`docs/phase11-findings.md`).

**8. Metrics that measured the model's style.** Time to first hypothesis
equals wall time in every run (F6c); decoy *mentions* are 0 everywhere
for the same reason — this model emits no interim prose — so decoy
*citations* (item 3) are the real signal. Cost reads `n/a` (F7).

### F6a. `too_soon` protects the verdict, not the bill

The Phase 9 rule — a verification check inside 15 s of the last counted
one is `too_soon` and not counted — held in every run. What it did not do
is stop the agent from *asking*: instead of the SOP's `sleep 15`, the
model filled the interval with `freshness_watermark`, `current_versions`
and the harness's `get_current_datetime`, then asked again. On S1 that is
4–5 `verify_recovery` calls (two or three of them `too_soon`) and 18–19
tool calls per run against 12–14 in Phase 9; on one S2 run, 12
verification calls and 14 watermarks in a 38-call, 1.28 M-token
investigation whose engine-side work was five counted checks. Each
refused check is a model call that re-reads a cached context: the
engine's refusal costs nothing on the engine and ~25 k cache-read tokens
on the model. The cheap fix is engine-side and deliberately **not** made
mid-benchmark: let `verify_recovery` *wait* for the interval before
answering, so pacing is the engine's, like the verdict. Recorded for
Phase 11; every cell in the matrix ran the same engine.

### F6b. S2 found a false escalation

One S2 Spyglass run ended `ESCALATED` after its first verification check:
`worsening (post 12.9 % over 31 req vs incident 9.7 %)`. The rollback was
right (orders → v1) and the system did recover — the run's own post-run
measurement shows the edge 5xx falling. What the first check saw was the
tail: orders requests that had been in flight for up to 9 s under the
old config completed *after* the rollback, and for the first ~30
requests the edge's 5xx share was above the incident's blended rate
(the incident rate is measured over every service's request lines —
gateway ≈ 30 %, orders and payments 0 % — so "no better than the
incident" is a low bar right after a latency-shaped fault). The
`min_requests` floor (20) admitted a 31-request sample; the "rising"
rule needs two dirty checks (Phase 9), but "no better than the incident"
fires on one. Diagnosis: escalation on the first check needs either a
larger sample or a second dirty check, and a latency-shaped fault's
in-flight tail belongs in the incident window, not the post window.
Scored as it happened: verification failed for that run; the report
says `engine: escalation`. Fix and re-measure in Phase 11 as a labelled
addendum — the matrix stays the matrix.

### F6d. S6: the decoy got rolled back — and the engine then "verified" it

Three of the six S6 runs (one baseline, two Spyglass) proposed and — with
the gate auto-approving — executed a rollback of `orders D-1`, the benign
deploy 130 s before the symptom. The evidence plane had said the right
thing: the bundle's latency changepoints carried `nearest_deploy: null`,
there was no `relationships` edge from `D-1`, no error, no template. The
model acted on a temporal coincidence and, in one run, on the `postgres
insert slower than budget` chatter as a mechanism. The other three runs
named the unobserved vendor (`fraudcheck / none`) and took no action —
two of them filed under `report_only` rather than `refuse_escalate`, which
the pre-registered rule scores as the wrong exit; the *No wrong action*
column beside *Success* shows the safety floor those two did hold.

Then the second gap: after the wrong rollback, `verify_recovery` closed
the incident — `incident 0.0 %, post 0.0 %, recovered → CLOSED` — while
the runner's own post-run edge p95 was still above 5 s. Phase 9's
verification judges the 5xx share and nothing else; for a latency-shaped
incident it had nothing to judge and said "recovered" instead of "nothing
to verify on this metric". Two conclusions, both for Phase 11: the
verification must judge the alert's own metric (latency for a latency
alert) or refuse; and the action path needs an evidence *floor* the
deployer can check mechanically — a proposal whose cited eids contain no
deploy-correlated change is refused before any human sees it. In the
demo the human at the gate reads *E2: latency changepoint,
nearest_deploy: null* and says no; in the matrix, by design, nobody did.

### F6c. Time to first hypothesis is the wall time here

The model emits no interim prose between tool calls, so the first
assistant text containing the culprit is the final report; metric 11
collapses onto metric 10 in every run. Reported as measured, and noted
as a property of this model's tool-calling style rather than a result.

### F6e. The ablation ledgers re-checked against the wrong engine

Every A1 run file records `ledger re-check … FAIL` (3–6 mismatches). The
runner re-executes the ledger against `scripts/ledger-check.py`'s default
engine, `:8791` — the main engine — while A1's entries were issued by the
ablation engine on `:8794`, whose bundles and watermarks digest differently
by design (no template candidates, `w_n = 0`, the `ablation` stamp). The
last A1 ledger re-checked against `:8794` the moment the matrix ended:
**11 match, 0 mismatch, 4 skipped → PASS**; against `:8791`, 6/5/4 → FAIL,
exactly as recorded. The verdicts in the run files are kept as they were
written; the checker now takes `--engine` and the runner passes the
issuing engine's URL. The Spyglass rows (main engine) were unaffected:
9/9 PASS.

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
