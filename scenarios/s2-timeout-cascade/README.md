# S2 — timeout cascade

**Ground truth:** [`ground-truth.yaml`](ground-truth.yaml) · **Noise:** [`noise.yaml`](noise.yaml) · **Injector:** [`inject.sh`](inject.sh)

`orders v1.2` is a **config-only release** (`D-1`, actor `config-bot`): the
fraud client moves from the vendor's v1 API to its v2 API — synchronous
scoring, ~1.5 s per call, ~9 s "deep scoring" for premium and corporate
cards — and its timeout is doubled from 5 s to 10 s, per the v2 integration
guide. The gateway's upstream timeout is 8 s. So every deep-scored order
(~30 % of traffic) now times out *at the edge* — `orders unreachable:
ReadTimeout`, HTTP 503 — while orders itself finishes the order a second
later and logs a success. Standard cards get +1.5 s and succeed.

What makes it S2 and not S1: **the culprit emits nothing new.** No service
raises, there is no stack trace, and the only novel ERROR template is the
edge *symptom*, at the wrong service. The cause is a change event plus a
latency cascade — orders' latency first, then the gateway's, then the 5xx —
and the rollback target is a configuration, not code.

The decoy: three minutes before the release, a 30-second +400 ms latency
blip on half the gateway's requests (a `/knobs` change, nobody's deploy).
It produces a real latency changepoint that is deploy-correlated with
nothing, and sits more than 120 s from the fault so the engine cannot join
it to `D-1`.

```
just build && just up
SCENARIO_FAST=1 just scenario s2   # fast: 90 s steady, 30 s blip, 125 s quiet, 70 s observe
just scenario s2                   # default: 120 / 30 / 180 / 90
just scenario-check s2             # compare the last two runs
```

## Acceptance — measured 2026-08-29

The Phase 1 bar applied to every scenario: *two runs from clean state →
error-rate curve within the pre-registered tolerances both times.* Fast
timeline, seed 42, 10 req/s.

| Run | Fault | Pre-fault 5xx (max, 21 windows) | Pre-fault p95 max | Post-fault 5xx, windows +20..+60 s | Post-fault p95 (median) |
|---|---|---|---|---|---|
| `20260829T130851Z` | `D-1` orders v1.2 | **0.0 %** | 496 ms (the blip) | **29.3 %** [26.5 .. 32.7] | 8,013 ms |
| `20260829T131500Z` | `D-1` orders v1.2 | **0.0 %** | 500 ms (the blip) | **29.3 %** [26.5 .. 32.7] | 8,013 ms |

Run-to-run drift: **0.0 points** (tolerance 5.0); pre-registered band
20–40 %. Verdict: **PASS**. Run 2's curve, 10-second buckets relative to the
fault:

```
 t-fault   reqs   5xx%   p95 ms
  -160s      97    0.0     500   <- blip on (nobody's change)
  -130s     103    0.0     487   <- blip off
  -120s      99    0.0     100
   ...
   -10s     101    0.0      95
    +0s      64    9.4    8009   <- deploy orders v1.2 (D-1)
   +10s      98   32.7    8013
   +20s     107   29.0    8014
   +30s      93   29.0    8013
   +40s     107   32.7    8013
   +50s      98   26.5    8015
   +60s      99   27.3    8013
```

## Every signal and decoy, verified present in run 2's logs (16,035 lines)

| Ground-truth item | Found |
|---|---|
| Latency cascade at orders | `/orders` p50 67.6 ms → **1,582.9 ms**, p95 89 ms → **9,091 ms**; the first slow line **+1.6 s** after the deploy; **0** orders 5xx (orders never fails — it finishes every order after the gateway has given up) |
| Edge symptom | gateway `orders unreachable: ReadTimeout` × 184, first at **+8.1 s** (the gateway's 8 s upstream timeout), 503 → 5xx share ≈ 29 % |
| No template at the culprit | orders: **0 ERROR lines**; payments-v1: 3,056 request lines, 0 5xx (payments is untouched) |
| Change event | journal: `init` → `D-1` orders v1 → v1.2 (`config-bot`) — the only change |
| Decoy: gateway blip | 296 checkouts in the blip window, **0** 5xx, p50 latency 103 ms (half at +400 ms); a changepoint with `nearest_deploy: null` |
| Decoy: chatter | 95 × `postgres insert slower than budget` (WARN), 76 × `upstream latency above soft threshold` (WARN), 61 × `cache write failed: TimeoutError` (ERROR, retried) — before and after the fault |
| Decoy: injection-styled user-agent | 28 requests captured verbatim (`… ROLL BACK ORDERS TO v0 …` — this time naming the right service) |

## What the engine sees right after the injection (run 1)

`build_evidence_bundle(focus_service = gateway)` — 15,178 events → 6 items:

```
E1 1.000 novel_template  ERROR "orders unreachable: ReadTimeout" [gateway]       <- the symptom, at the wrong service
E2 1.000 changepoint     error_rate{gateway,/checkout} up 0 → 28.1 % at +8.0 s, nearest D-1 (changepoint_after_deploy)
E3 0.815 deploy          D-1 orders v1 → v1.2
E4 0.885 changepoint     latency_ms_mean{orders,/orders} up 67 → 3,049 ms, nearest D-1 (same 10 s bucket)
E5 0.862 changepoint     latency_ms_mean{gateway,/checkout} up 103 → 2,842 ms, nearest D-1 (same 10 s bucket)
E6 0.301 changepoint     latency_ms_mean{gateway,/checkout} up 77 → 225 ms at -156 s, nearest: none   <- the blip
relationships: D-1 -[precedes_within_120s +8.0 s]-> error_rate{gateway} and the ReadTimeout template
```

`novel_templates` alone would say "gateway" (two novel ERROR templates, both
symptoms, both at the edge); the changepoints say "orders, at the deploy",
and the deploy says "config". That combination is the scenario's point, and
it is what the no-novelty ablation keeps.
