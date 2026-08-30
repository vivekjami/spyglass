# ADR-008 — Evidence ranking as a hand-weighted linear model

**Status:** Accepted · **Date:** 2026-08-28 (expanded at Phase 6, when the ranker was built)

## Context

The bundle has a budget — twenty items, eight kilobytes — and far more
candidates than that: every novel template, every changepoint, every deploy
in the window. Something has to order them, and whatever does must be
explainable factor by factor in the ledger, because a ranking the agent
cannot inspect is a ranking the agent cannot argue with.

## Decision

- **A linear model over six factors, each in [0, 1]:**
  `score = w_n·novelty + w_t·proximity + w_s·severity + w_d·deploy_correlation + w_f·freq_shift + w_r·relevance`.
  The weights are the spec's v0 (`.30 .15 .10 .25 .10 .10`), in
  `spyglass.toml [ranking]`, overridable per call for ablation runs, and
  **recorded in the ledger args of every bundle call** with each item's
  weighted contributions on the item itself.
- **Factors are defined the same way for every kind**, so kinds compete on
  one scale:
  - *novelty* — did this behaviour first appear in the window? A template
    first seen there: 1.0; an error series going from zero: 1.0; a burst:
    the shared `log2(ratio)/6` mapping; a deploy: 1.0 (a new event).
  - *proximity* — `exp(−|t − T0| / 120 s)`, where T0 is the engine's onset
    estimate: the earliest error changepoint, else the earliest novel ERROR
    template, else the window end. Reported on the bundle.
  - *severity* — templates ERROR 1.0 / WARN 0.5 / INFO 0.0; changepoints
    error series up 1.0, latency up 0.6, traffic or any down step 0.3;
    deploys 0.5.
  - *deploy_correlation* — 1.0 when a change event of another kind lies
    within ±120 s (bounded by the window end), else 0.
  - *freq_shift* — the magnitude mapping; "from zero" / first seen = 1.0.
  - *relevance* — `0.75^hops` from the focus service over the config
    topology (instances resolve to their service; a cascade takes the max
    of its members); 1.0 for everything when no focus is given.
- **Dedupe before scoring.** An error that propagates through the call
  chain is one fact: templates whose exemplar events share request ids are
  a cascade, reported once with the origin (earliest first seen, then the
  stack trace) as the item and the rest inside it; so are error-series
  changepoints on connected services within `cascade_secs` (2 s) of each
  other. S1's five novel ERROR templates and four error changepoints are
  two facts.
- **The bundle's head is kind-diverse.** After scoring, the best template,
  the best changepoint and the best deploy come first (in score order),
  then everything else by score. An incident is what changed, when, and
  what was deployed; a bundle that opens with three templates has answered
  one question three times. Each item carries `score_rank`, its position
  by score alone, so the head's effect is never hidden.
- **Ties** break by kind (template, changepoint, deploy), then time
  ascending, then the stable ref.

## What the weights do and do not do on S1 (measured, Phase 6)

With focus `gateway`, fault window: root template 1.000, error changepoint
1.000, fault deploy 0.805, INFO decoy template 0.856, loadgen's periodic
INFO line 0.802, benign deploy 0.773.

- The three key facts are the top three — because of the head. **By score
  alone the INFO decoy (novel, harmless, 0.856) outranks the fault deploy
  (0.805).** With `w_s = 0.10`, severity cannot outrank novelty; the spec's
  scenario note ("severity must outrank novelty") is satisfied by the head
  rule, not by the weights. Recorded here rather than tuned away: the
  weights are the spec's v0 and Phase 6 had one scenario to tune on, which
  is not tuning.
- **`w_n = 0` does not reorder S1's bundle.** Every candidate in the incident
  window is first-seen — templates, the from-zero changepoint, the deploys
  — so novelty is a constant 0.30 there and zeroing it lowers every score
  by exactly 0.30. The spec's Phase 6 acceptance ("toggling `w_n = 0`
  visibly reorders") is met in the form the plumbing can actually prove on
  S1: every score moves by `w_n`, the ledger records the weights, and the
  check reports the order change or says why there is none. A window with
  mixed novelty (a bursting old template beside a new one — S3 is built
  that way) is where the order moves.
- What separates the decoy from the deploy is `severity` and `freq_shift`;
  what separates the benign deploy from the fault deploy on the default
  timeline is `deploy_correlation` and `proximity` (0.43 vs 0.81); on the
  90 s fast timeline `D-1` sits within 120 s of the changepoint and scores
  0.773 — a true statement about that timeline.

## Alternatives considered

- **Learning-to-rank.** Rejected: no training data, no explainability.
- **LLM-as-ranker.** Rejected: puts the model back on the hot path the
  engine exists to shorten.
- **Pure score order, no diverse head.** Rejected on S1's own evidence:
  the top three would be template, changepoint, INFO decoy, and the deploy
  fourth. The head is a diversity rule, not a weight, and it is visible.
- **Severity as a multiplicative gate.** Rejected for v0: it breaks the
  factor-by-factor additive explanation the ledger relies on.

## Consequences

- Rankings are opinions with receipts: every item says how it got its
  score, every ledger entry says which weights were used.
- The ablation (`w_n = 0`) is one argument, or one config line for a
  second engine instance in Phase 10.
- The kind-diverse head assumes three kinds; when a kind is absent (no
  deploy in the window) the head has two, which is correct.

## Reversal conditions

If S2/S3 (Phase 10) show the v0 weights ordering their key facts wrong by
score in a way the head does not cover — a latency changepoint under an
INFO template, say — tune `w_s` and `w_f` on those runs and commit the
runs that moved them. The scorer is a pure function; swapping it is
additive.
