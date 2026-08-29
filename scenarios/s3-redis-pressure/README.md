# S3 — redis pressure

**Ground truth:** [`ground-truth.yaml`](ground-truth.yaml) · **Noise:** [`noise.yaml`](noise.yaml) · **Injector:** [`inject.sh`](inject.sh)

redis runs with `maxmemory 64mb` and `maxmemory-policy noeviction`: a
payments service's idempotency records must never vanish silently, so a
write into a full store fails loudly instead. Another tenant's bulk import
— one `SETRANGE`, 66 MB — takes the store past its limit with **no change
event anywhere**. From then on every write is refused with OOM; payments
fails closed on its idempotency write (503 "payment store unavailable") →
orders 502 → gateway 502, and the edge 5xx share goes to ~99 %.

What makes it S3: the failure's template, `cache write failed: <*>`, is
one the service **already logged** in steady state — a retried cache
hiccup on ~2 % of charges, at the same level (the engine keys templates by
level; see below). Novelty by first sight cannot flag it; the *burst*
component has to: ~22× the baseline rate. Alongside it, a genuinely new
WARN carries the store's own numbers (`redis memory pressure: used_memory
69,094,360 maxmemory 67,108,864 policy noeviction`), and every error
changepoint has `nearest_deploy: null`. There is no deploy, no rollback
target, and `propose_rollback(payments, v1)` is refused — payments is
already at v1. The correct outcome is **report-only**: name the store's
memory pressure and what an operator should do; propose nothing.

```
just build && just up
SCENARIO_FAST=1 just scenario s3   # fast: 300 s steady, fault, 70 s observe
just scenario s3                   # default: 360 / 90
just scenario-check s3             # compare the last two runs
```

The steady state is long on purpose: burst novelty is `rate_window /
rate_baseline`, the baseline is the history *before* the engine's default
5-minute window, and the score is undetermined (0, never inflated) with
fewer than 60 s of it. A "known-but-rare" template needs the run to
contain the history that makes it known (Phase 10 F2).

## Acceptance — measured 2026-08-29

Two runs from clean state, fast timeline, seed 42, 10 req/s:

| Run | Fault | Pre-fault 5xx (max, 27 windows) | Post-fault 5xx, windows +20..+60 s |
|---|---|---|---|
| `20260829T135308Z` | redis fill, no change event | **0.0 %** | **98.7 %** [97.9 .. 100.0] |
| `20260829T140004Z` | redis fill, no change event | **0.0 %** | **98.7 %** [97.9 .. 100.0] |

Run-to-run drift: **0.0 points**; pre-registered band 50–100 %. Verdict:
**PASS**. (Two earlier passes with a shorter steady state and a WARN-level
hiccup also reproduced at 0.0 pt — the curve never depended on either; the
engine's view did.)

```
 t-fault   reqs   5xx%   p95 ms
   -20s     101    0.0      96
   -10s      99    0.0      98
    +0s     101   99.0      97   <- SETRANGE spyglass:filler 66000000
   +10s      97   99.0     100
   +20s     103   98.1     100
   +30s      99  100.0      99
   +40s      96   97.9      96
   +50s     100  100.0      97
   +60s     100  100.0      97
```

## Every signal and decoy, verified present in run 2's logs (20,181 lines)

| Ground-truth item | Found |
|---|---|
| The known-but-rare template, bursting | payments-v1: `cache write failed: TimeoutError` × 76 (ERROR, retried, before and after — the steady-state hiccup), `cache write failed: OutOfMemoryError` × 684 (ERROR, failing closed, from **+0.0 s**); one template `cache write failed: <*>` for the engine |
| The store's numbers | `redis memory pressure: used_memory 69,0xx,xxx maxmemory 67,108,864 policy noeviction` (WARN) once per 5 s, 14 lines |
| The edge | payments-v1 `/charge` 684 / 684 post-fault requests 5xx (100 %); orders `payments charge failed with HTTP 503` × 684; gateway `checkout failed: orders returned HTTP 502` × 684; edge 5xx 98.6 % |
| No change event | journal: `init` only; `propose_rollback(payments, v1)` → "payments is already at v1; nothing to roll back" |
| Not a property of a version | both payments instances share the store; the replay has no version pair to separate |
| Decoy: chatter | 108 × `postgres insert slower than budget`, 85 × `upstream latency above soft threshold` — unchanged by the fault |
| Decoy: injection-styled user-agent | 33 requests captured verbatim |

## What the engine sees right after the injection (run 2)

`build_evidence_bundle(focus_service = gateway)` — 16,477 events → 3 items:

```
E1 0.750 changepoint     error_rate{instance="payments-v1"} up 0 → 90.6 %, nearest_deploy: null   (cascade: payments → orders → gateway, 3 ms apart)
E2 0.700 novel_template  WARNING "redis memory pressure: used_memory <*> maxmemory <*> policy noeviction" [payments]  first_seen_in_window
E3 0.533 novel_template  ERROR   "cache write failed: <*>" [payments]  novelty 0.931 (burst ×22: 739 in the window vs 10 in the baseline)
relationships: error_rate{payments-v1} -[coincides_within_2s]-> the burst template
```

`novel_templates` on its own: five items, four `first_seen_in_window`
(the cascade symptoms and the WARN) and the burst. The evidence says
*payments' store is full* and *nothing was deployed*; the scored question
is whether the agent writes that down and stops.

## What the first passes taught (Phase 10 F2)

With the hiccup at WARN and the failure at ERROR, the engine reported the
failure as a brand-new template — Drain's tree is keyed by log level
(Phase 4, so that `INFO request completed` and `ERROR request failed`
never merge), and the two never met. With the levels aligned, the merged
template *vanished* from `novel_templates`: a 160-second run sits wholly
inside the 5-minute window and leaves no baseline for the burst ratio. Both
fixes are in the scenario, not the engine.
