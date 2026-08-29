# Phase 5 — Changepoint detection: build record

**Objective (spec):** `detect_changepoints` timestamps the incident boundary
and annotates the nearest deploy. The metrics analyst's finding.
**Built:** 2026-08-29, 09:55–14:00 IST (04:25–08:30 UTC; the calendar in
`progress.md` uses IST) · **PR:** #6
**Acceptance bar (spec):** S1 changepoint within ±10 s of injected truth;
annotated with the fault deploy; no changepoints on 10 minutes of steady state.

---

## Status summary

| Spec task | Status | Where |
|---|---|---|
| Guarded rolling baseline | ✅ 15 min baseline, 30 s guard (spec said 120 s — F2), σ floors per series kind, ≥ 6 real baseline buckets or *undetermined* | `crates/spyglass-engine/src/changepoints.rs` |
| z-score detector | ✅ `|z| ≥ 4` for ≥ 2 consecutive buckets, one item per run; pure function, 9 unit tests | same |
| Deploy-correlation join | ✅ nearest journal entry within ±120 s with `offset_secs` and a precision-aware `relation` (F4) | `tools.rs::detect_changepoints` |
| Wire tool | ✅ `detect_changepoints(metrics, service, route, window, baseline, limit)`; `just s5-check` runs the acceptance | `spyglass-mcp`, `scripts/changepoint-check.py` |
| **Acceptance: S1 changepoint ±10 s, annotated with the fault deploy** | ✅ **+0.5 s** on the default timeline, **+0.6 s** on the fast one; `nearest_deploy: D-2`, `changepoint_after_deploy` (F3) | |
| **Acceptance: 10 min steady state → nothing** | ✅ 10.7 and 11.5 idle minutes after a rollback → 0 changepoints (32 × 88 and 20 × 92 series × buckets); 12 idle minutes earlier → 0 over 32 × 97; the decoy deploy window (7 min, `D-1` inside) → 0. Measured on an idle host, because the detector is right about a host that is not idle (F5) | |
| SOP v3: `detect_changepoints` in the triage, `nearest_deploy.relation` in the contradiction check | ✅ 625 words | `agent/sop.md` |
| Thresholds in config, tuned on S1 only, said so | ✅ `spyglass.toml [changepoints]` | |

Deferred per spec: CUSUM (unless S4 demands it in Phase 10).

---

## Findings and decisions

### F1. The series come from the logs, not the scraper

The spec's diagram feeds the detector from the metrics scraper. The detector
built here reads the **request lines** instead — every `request completed` /
`request failed` line carries service, instance, route, status and
latency — and computes `error_rate`, `errors_total`, `requests_total` and
`latency_ms_mean` per service, per service+route and per instance on 10 s
buckets aligned to the epoch. On S1 that is 32 series (20 once a single-route service's aggregate folds into its route series, F4.2).

Why: the scraped Prometheus counters are stamped with the scrape's wall clock
and live in an in-memory ring; a changepoint computed from them is neither
event-time precise nor reproducible after an engine restart. The log-derived
series are event-time stamped and rebuilt from the files on start, so
`detect_changepoints` is **deterministic on frozen data** and its ledger
entries re-check like every other read tool (ADR-004). The scraper stays:
ingested and watermarked for freshness, and the input for any series the
logs cannot express. ADR-007 records the decision; the README's C4 and
diagram are corrected.

### F2. The guard is 30 s, not 120 s — because the fast timeline has 90 s of history

The spec's guard excludes the last 2 minutes from the baseline. On the
demo's fast timeline the fault lands 90 s after traffic starts: a 120 s
guard leaves **no** baseline, and the tool reports nothing at the moment the
demo needs it. The guard's job is to keep a change out of its own baseline
while it is being confirmed — two buckets, 20 s. 30 s does that with margin
and leaves six baseline buckets on the fast timeline. Keeping a change
*flagged* for minutes afterwards is `error_delta`'s job. Recorded in
`spyglass.toml` next to the value and in ADR-007.

### F3. Acceptance, measured

**Default timeline** (`20260829T043834Z`: 120 s steady, `D-1` orders v1.1 at
04:40:34.857, 360 s lead, `D-2` payments v2 at **04:46:34.876**, 90 s
observed). Window `[D-2 − 60 s, end]`, 32 series, engine 84 ms:

| # | `at` | vs truth | series | change | z (first / peak) | nearest deploy |
|---|---|---|---|---|---|---|
| 1 | 04:46:35.368 | **+0.5 s** | `error_rate{payments,/charge}` (+`errors_total`) | 0.0 % → 17.8 %, from zero | 7.2 / 20.1 | `D-2` +0.5 s, after |
| 2 | 04:46:35.368 | +0.5 s | `error_rate{payments}` | same | 7.2 / 20.1 | `D-2` +0.5 s |
| 3 | 04:46:35.368 | +0.5 s | `errors_total{instance=payments-v2}` (+`requests_total` from zero at +0.1 s) | 0 → 18 / 10 s | 7.0 / 18.9 | `D-2` +0.5 s |
| 4 | 04:46:35.373 | **+0.5 s** | **`error_rate{orders,/orders}`** — the ground-truth series | 0.0 % → 17.8 %, from zero | 7.0 / 19.9 | **`D-2` +0.5 s, after** |
| 5 | 04:46:35.373 | +0.5 s | `error_rate{orders}` | same | 7.0 / 19.9 | `D-2` +0.5 s |
| 6 | 04:46:35.376 | +0.5 s | `error_rate{gateway,/checkout}` | same | 7.0 / 19.9 | `D-2` +0.5 s |
| 7 | 04:46:35.376 | +0.5 s | `error_rate{gateway}` | same | 7.0 / 19.9 | `D-2` +0.5 s |
| 8 | 04:46:40.000 | +5.1 s (bucket start) | `requests_total{instance=payments-v1}` | 94 → 0 / 10 s | −6.1 / −6.2 | `D-2` +5.1 s |

The ground-truth series is localised to **0.5 s** (spec tolerance ±10 s,
ground truth ±15 s), magnitude "from zero" (pre-fault 5xx rate is exactly
0.0 %, so the ratio is undefined and the tool says so rather than dividing by
a floor), nearest deploy `D-2`, `relation: changepoint_after_deploy`. The
first three items are the origin (payments), the next two the victim
(orders, 5 ms later), then the edge (gateway, 3 ms after that) — the cascade
in causal order, from `at` ascending alone. Item 8 is the traffic *leaving*
payments-v1 5 s later (a down step keeps the bucket start; F4).

**Decoy window** — the 419 s before the fault, with the benign `D-1` deploy
inside: **0 changepoints** on 28 series. `D-1` is annotated on nothing.

**Fast timeline** (`20260829T040927Z`, the demo path): the same series at
**+0.6 s**, z 4.0 first / 23.2 peak, `D-2` +0.6 s; 9 items; decoy window
(29 s) clean. The first flagged bucket held only 1.4 s of post-fault traffic,
hence z = 4.0 — barely confirmed there, confirmed regardless by the next
bucket. The traffic-leaving-v1 step is **not** reported on this timeline:
the loadgen's ramp-up bucket is one of only six baseline buckets and
inflates σ to 27, so the drop scores z = −3.7. Recorded, not tuned away
(ADR-007, "robust σ").

**Demo run** (`20260829T074618Z`, `just demo`, fast timeline, `D-2` at
07:47:48.600): the fault landed 8.6 s into a bucket, so the first bucket had
too little exposure and the next one confirmed — `error_rate{orders,/orders}`
at **+1.4 s**, z 21.8, `D-2` +1.4 s; decoy window clean; and the spec's
steady-state check on the **10.7 minutes after the agent's rollback, idle
host: 0 changepoints over 32 series × 88 buckets**. `just s5-check` → PASS
on all three. Repeated on the second agent run's data with the final binary
(`20260829T080250Z`): `error_rate{orders,/orders}` **+0.6 s**, z peak 25.3,
`D-2` +0.6 s, rank 3 of 4 behind the two payments items; decoy clean; 11.5
idle minutes after the rollback → **0 changepoints over 20 series × 92
buckets**. PASS.

**Steady state, twice more** — 12 build-free minutes after the Phase 4
rollback (`04:13:30–04:25:30`): **0 changepoints** over 32 series and 97
buckets, no unconfirmed tail; the default run's 419 s decoy window: 0.

**Recovery as a changepoint** — with `baseline` set to the incident period
and the window after the rollback, the tool reports `error_rate{orders,
/orders} down 19.4 % → 0.0 %` at the rollback bucket, nearest deploy `D-3`,
`relation: same_bucket_order_unresolved` — the rollback landed inside that
bucket, and a bucket-start `at` refuses to claim it came first (F4).

### F4. Three honesty fixes the first outputs forced

1. **A bucket boundary must never say "before the deploy".** A down step
   (traffic leaving v1) has no first anomalous event, so `at` is the bucket
   start — which can precede a deploy that landed inside the same bucket by
   up to 10 s. The SOP's contradiction check asks exactly "does the
   changepoint precede the deploy?"; a bucket edge would have answered *yes*
   falsely. Now `at_precision` is reported and the deploy `relation` honours
   it: event-precise `at` orders exactly; bucket-start `at` yields
   `same_bucket_order_unresolved` for a deploy inside the bucket. Up steps on
   error and traffic series are refined to the first 5xx / first request in
   the bucket, which is how the +0.5 s comes out.
2. **Sixteen items said six things.** `error_rate` and `errors_total` on
   the same labels moved in the same bucket; so did service-level and
   route-level series for single-route services. Grouping by (label set,
   bucket, direction) with the normalised rate speaking for the group and
   the rest in `also_changed` took S1 from 16 items to 9, and folding a
   single-route service's aggregate into its route series (strictly more
   information, same fact) took it to **6**. Items had been sitting *at* the
   2048-byte cap — `cap_item` was halving the headline — and are now ≤ 1.1 kB
   each after dropping what `get_evidence` or a narrower query answers
   better (per-bucket z and value, raw σ, label map, bucket bounds): the
   whole response is **7.0 kB**, down from 16.2 kB in the first agent run
   (F6).
3. **The deploy join is bounded by the window.** The first agent run's
   ledger re-check failed on `detect_changepoints`: the agent's own rollback
   `D-3` landed 17 s after the call's window ended and, within ±120 s of the
   changepoints, joined on replay — the Phase 3 `deploy_events` lesson in a
   new place. Only journal entries with `ts ≤ window.to` join now; a deploy
   after the evidence window is outside the evidence. Re-check: F6.

### F5. The detector is right about a host that is not idle

The first steady-state run flagged a 1.9× latency doubling on gateway and
orders at 04:28:20, two buckets long, "no deploy within ±120 s". The gateway
log confirms it: mean /checkout latency 60 → 125 → 103 → 61 ms across four
buckets — while `cargo test` and `cargo build --release` were loading the
same host (the test binary's mtime is 04:28:16). Later, the default-timeline
run's long post-fault window flagged latency at 05:15, traffic collapsing
98 → 5 → 0 requests / 10 s at 05:22:50, and traffic resuming from zero at
07:42:37 — the laptop suspended for two and a half hours. Every one of those
is a real change in the series, each correctly annotated with no nearby
deploy, which is the README's own failure-mode row ("no nearby change event
→ deprioritised") working as designed. The acceptance "no changepoints on
steady state" is therefore measured on an *idle* host, and the run files
that were not idle are kept, not deleted.

### F6. The agent runs

Two `just demo` runs with SOP v3, the same fault, the same approval policy.
Run 1 against the first tool shape (16.2 kB response, 10 items); run 2 after
F4.2–F4.3.

| Metric | P5 run 1 | P5 run 2 | Phase 4 (novelty only) | Baseline |
|---|---|---|---|---|
| Outcome | completed: correct RCA, `D-3` citing 5 eids, **20.6 % → 0.0 %** | completed: correct RCA, `D-3` citing E2/E3/E9/E10, **20.6 % → 0.0 %** | completed | completed |
| Tool calls | 13 (`detect_changepoints` 1 added to P4's 12) | 13 | 12 | 19 |
| Model calls | 11 | 11 | 9 | 11 |
| Input tokens | 259,677 | **222,647** | 141,601 | 198,106 |
| Output tokens | 8,827 | 8,186 | 7,551 | 4,978 |
| Peak context | 29.8k | 26.4k | 21.5k | 30.0k |
| Tool bytes → context | 47,148 (of which `detect_changepoints` 16,221) | 38,133 (of which `detect_changepoints` **6,285**) | 31,363 | 57,147 |
| Wall | 82.6 s | 92.7 s | 70.5 s | 39.5 s |
| Evidence ids cited | 21 of 23 | 16 of 18 | 14 of 14 | none exist |
| Ledger re-check | **FAIL 4/5** (F4.3) | **PASS 5/5** | PASS 4/4 | n/a |

**What the tool bought.** Run 1's postmortem timeline is the metrics
analyst's finding in prose, every line with an eid: *"Traffic changepoint:
`requests_total` for `payments-v2` increased from 0 to 82 req/10s (+0.1 s
after D-2) [E8]"*, *"Error rate changepoint on `orders /orders` spiked from
0.0 % to 20.2 % (+1.4 s after D-2) [E14, E15]"*, and the correlational label
carries the offset: *"CORRELATIONAL (+1.4 s offset between deploy D-2 and
error changepoints [E11, E16])"*. `D-1` is rejected with the fact that error
rates stayed at 0.0 % for 50 s after it `[E11, E14, E16]`. That is what the
spec's "+118 s after deploy" line was for.

**What it cost.** Run 1's input tokens went **up 83 % against Phase 4**. The
16.2 kB changepoint response sat in the context of every one of eleven model
calls — peak context 29.8k against 21.5k — and the model spent two extra
calls (one of 29 output tokens) reading it. Bounded evidence is not just a
cap; the *default* shape has to be lean, because a read tool's response is
paid for once per subsequent model call, not once. Hence F4.2.

**Run 2, the honest number.** With the 6.3 kB response: 222,647 input
tokens — 14 % below run 1, ledger re-check **PASS 5/5**, the same postmortem
shape (*"Earliest metric changepoint detected: `error_rate` on payments
/charge jumped from 0.0 % to 20.3 % [E9] … +0.6 s after D-2"*; the label:
*"Correlational — the error changepoint occurred +0.6 s after deploy
D-2"*). But still **57 % above Phase 4's 141.6k, and 12 % above the
baseline**. The reason is structural, not a bug: on S1, `novel_templates`
already answers the question, so a second tool that answers it again from
a different series adds context to every later call and two model calls
of reading, and returns a *better-evidenced* answer, not a cheaper one.
Where `detect_changepoints` should pay for itself is S2, whose fault
produces **no** novel error template and only a latency cascade — that is
the spec's own discriminating scenario, and it is Phase 10's measurement,
not this phase's. On S1 the tokens-vs-baseline claim now rests on Phase 4's
run alone (n = 1), and the SOP may want to make the changepoint call
conditional on the novelty result not being decisive; recorded as the
first thing to try in Phase 6/7, where the bundle replaces both calls.

### F7. Things that pushed back

- `cap_item` silently halved the headline string when an item crossed
  2048 bytes; the fix was to make items smaller, not the cap larger (F4.2).
- The explicit-baseline mode ("compare every bucket to the incident
  period") initially dropped a recovery that *began* before the query
  window, because rolling mode's "began inside the window" rule applied to
  both. Explicit mode is a state comparison; a run still in progress when
  the window opens now counts and says `began_before_window: true`.
- IST timestamps in earlier phase records were computed loosely; this
  record and the progress table use UTC+5:30 from the commit and log times.
- The laptop suspended for 2.5 h in the middle of the phase with the fault
  active; the default-timeline run's data survived (files, not processes),
  the engine rebuilt from them on restart, and the detector reported the
  suspend as what it was (F5).

---

## Reproducing this

```bash
just build && just mcp-up && just tf-setup     # engine now serves detect_changepoints
cargo test --release -p spyglass-engine        # 23 tests: Drain + novelty + changepoints
just scenario s1                               # default timeline, ≈9.5 min; or S1_FAST=1
just s5-check                                  # fault ±10 s + D-2, decoy clean, steady clean (needs ≥10 idle min after the last deploy, or --steady from,to)
DEMO_APPROVAL=allow just demo                  # Spyglass with SOP v3
```

---

## Spec revisions this phase forces

1. **C4's baseline** is `[−15 m, −30 s]`, not `[−15 m, −2 m]`, with σ floors
   and an *undetermined* outcome below six baseline buckets (F2).
2. **C4's input** is the log-derived request series, not the scraper; the
   diagram's arrow moves (F1, ADR-007).
3. **`at` carries a precision**, and the deploy relation is precision-aware
   (F4.1); the tools table gains `baseline` (recovery check) and `route`.
4. **The steady-state acceptance** must state the host was idle (F5).
