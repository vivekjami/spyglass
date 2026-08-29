# ADR-007 — Changepoint detection via a guarded rolling z-score first

**Status:** Accepted · **Date:** 2026-08-28 (expanded at Phase 5, when the detector was built)

## Context

The agent needs "when did behaviour change", cheaply, on streaming data, in
a form it can cite: a series, a timestamp, a magnitude, and the nearest
change event. It does not need the statistically optimal segmentation of a
time series; it needs a boundary it can put next to a deploy timestamp. The
spec's Phase 5 acceptance is the whole requirement: the S1 changepoint within
±10 s of the injected truth, annotated with the fault deploy, and *nothing*
on ten minutes of steady state.

## Decision

- **Detector v0: rolling z-score on 10 s aggregates.** For each bucket,
  `z = (x − mean) / max(σ, floor)` over a trailing baseline that *excludes a
  guard interval* before the bucket, so a change cannot sit in its own
  baseline while it is being confirmed. A changepoint is `|z| ≥ 4` for `≥ 2`
  consecutive buckets with one sign; it is reported once, at the first
  flagged bucket. Thresholds, the guard, the baseline length and the σ floors
  live in `spyglass.toml [changepoints]`; they were tuned on S1 and on ten
  minutes of its steady state, and the config file says so.
- **σ floors.** A flat baseline has σ ≈ 0 and would turn the first wobble
  into `z = ∞`. Counts are floored at `max(1, √mean)` (Poisson), rates at one
  percentage point, latency at `max(2 ms, 5 % of the mean)`.
- **The series come from the logs, not the scraper.** Every request line
  carries service, instance, route, status and latency, so `error_rate`,
  `errors_total`, `requests_total` and `latency_ms_mean` per service, per
  service+route and per instance are computed from events. Event-time
  stamped, rebuilt from the files on every engine start — a changepoint is
  therefore deterministic on frozen data and its ledger entry re-checks
  (ADR-004). The scraped Prometheus counters are wall-clock stamped and live
  in an in-memory ring; they stay ingested and watermarked for freshness, and
  would be the input for a series the logs cannot express. The spec's
  diagram (`metrics scraper → changepoint detector`) is corrected to this.
- **Guard = 30 s, not the spec's 120 s.** The guard only has to keep the
  baseline clean through confirmation (two buckets); 30 s does that and
  still leaves six baseline buckets on the 90 s fast timeline the demo uses.
  A 120 s guard would leave *no* baseline there and the tool would report
  nothing. What a long guard buys — keeping a change flagged for minutes —
  is `error_delta`'s job, not the detector's.
- **`at` is refined inside the first flagged bucket** where "first anomalous
  event" is well defined: the first 5xx for an error series going up, the
  first request for traffic appearing. A drop is an absence and keeps the
  bucket start. The precision is reported (`at_precision`), and the deploy
  relation honours it: an event-precise `at` orders against a deploy
  exactly; a bucket-start `at` can never claim to *precede* a deploy that
  landed in the same bucket (`same_bucket_order_unresolved`). Without this a
  10 s bucket boundary could hand the agent's contradiction check a false
  "the change came before the deploy".
- **Deploy correlation is a join, not a claim.** Every changepoint carries
  the nearest journal entry within ±120 s with `offset_secs` and `relation`,
  and the rest nearby. The headline reads "+0.6 s after D-2 (payments
  v1→v2)". Causal language stays reserved for the controlled comparison
  (C9).
- **One item per label set, bucket and direction.** `error_rate` and
  `errors_total` moving together on the same labels are one fact; the
  normalised rate speaks for the group and the rest ride in `also_changed`;
  a single-route service's aggregate is folded into its route series.
  Sixteen items became six on S1, ≤ 1.1 kB each, 7 kB for the response —
  because a read tool's output is paid for once per *subsequent* model
  call, and the first agent run showed a 16 kB response costing 83 % more
  input tokens than the phase before.
- **The deploy join is bounded by the evidence window.** Only journal
  entries with `ts ≤ window.to` are joined. A rollback landing seconds after
  the call would otherwise appear on replay and break the ledger digest —
  it did, on the first agent run. A deploy after the window is outside the
  evidence; the agent re-queries with a later window to see it.
- **Explicit baseline = a state comparison.** With `baseline` given (the
  incident period, say), every bucket is scored against that fixed window,
  so "has it recovered?" is a `down` changepoint. A run still in progress
  when the window opens is reported with `began_before_window: true` rather
  than dropped — otherwise a recovery that started 60 s ago would read as
  "no changepoint" in a 30 s window. Rolling mode keeps the stricter rule:
  only changes that *began* inside the window; an older change is an older
  change, and the agent widens the window.
- **Ordering:** `at` ascending — the earliest change is the likeliest origin
  — then |z|, then series key. On S1 that puts payments /charge first,
  orders 5 ms later, gateway 4 ms after that: the cascade in causal order.
- **No CUSUM yet.** Ship z-score first; add CUSUM only if S4's slow drift
  demands it (Phase 10). The detector is a function over a bucketed series;
  a second one is additive.

## Alternatives considered

- **PELT / BinSeg (offline segmentation).** Rejected for v0: offline,
  harder to explain to an SRE at 3 a.m., marginal gain at demo scale.
- **Learned detectors.** Rejected: off-thesis. The point is that the
  evidence is computable, cheap and explainable *before* the model sees it.
- **Scraped counters as the input.** Rejected for v0 on determinism (above);
  not wrong, just not re-checkable after a restart.
- **A robust σ (MAD).** Not adopted, and noted: on the 90 s fast timeline the
  loadgen's ramp-up bucket inflates σ for `requests_total{instance=
  "payments-v1"}` enough that the traffic *leaving* v1 scores z = −3.7 and
  is not reported (it is on the default timeline: z = −6.1 against 47 baseline buckets). Switching
  estimators the moment one case misses would be tuning on S1; the miss is
  recorded instead, and MAD is the first thing to try if Phase 10 shows it
  mattering. The fault itself — errors from zero — is unaffected.

## Consequences

- Phase 5 acceptance on S1: the orders /orders error-rate changepoint 0.6 s
  after the injected deploy, magnitude "from zero" (baseline 0.0 %), nearest
  deploy `D-2` at +0.6 s; the benign deploy `D-1` annotated on nothing;
  twelve build-free minutes of steady traffic → zero changepoints across 32
  series and 97 buckets.
- The detector reports *true* changes that are not incidents: a 20 s
  latency doubling on gateway and orders while `cargo build` loaded the same
  host, correctly annotated "no deploy within ±120 s". That is the spec's
  failure-mode row working as designed — no nearby change event →
  deprioritised — and it is why the steady-state acceptance is measured on
  an idle host.
- Resolution is one bucket: a change that lands late in a bucket is
  confirmed in the next one, so `at` can trail the truth by up to 10 s (the
  spec's tolerance). Refinement to the first anomalous event recovers
  sub-second precision whenever the change is an *appearance*.
- Slow drifts (S4) will not cross `z ≥ 4` against a rolling baseline that
  absorbs them; that is CUSUM territory, deferred by design.

## Reversal conditions

If Phase 10's S2 (latency cascade) or S4 (pool leak) shows the z-score
missing a seeded change, add CUSUM as a second detector and report the miss.
If steady-state false positives appear on a quiet host, the σ floors and the
consecutive-bucket rule are the knobs — in config, with the runs that moved
them committed.
