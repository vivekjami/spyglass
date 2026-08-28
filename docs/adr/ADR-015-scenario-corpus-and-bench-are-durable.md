# ADR-015 — The scenario corpus and benchmark harness are durable artifacts

**Status:** Accepted · **Date:** 2026-08-28 (expanded at Phase 1, when the corpus was built)

## Context

The algorithms in Spyglass are reproducible from published work — Drain,
z-scores, weighted linear ranking. What is *not* reproducible from a paper is a
fault-scenario suite with pre-registered ground truth, deterministic traffic,
and a runner that scores an investigation against it. That is the asset that
compounds, and it is how ITBench — not any single agent — came to anchor the
field's discourse.

The temptation under a deadline is to hard-code the demo: one script that
breaks one thing and one prompt that finds it. That is unfalsifiable and
single-use.

## Decision

`scenarios/` and `bench/` are first-class components with their own contracts:

- **`scenarios/SCHEMA.md`** defines the ground-truth format: culprit entity and
  change id, expected evidence kinds, decoys, correct action (including "none"
  and "refuse"), error-rate tolerances, verification signal.
- **Ground truth is written before any run and committed with the scenario.**
  Deploy ids are deterministic from a reset journal, so the culprit change can
  be named up front (`D-2`).
- **Traffic is a pure function of a seed.** Every random draw a request needs
  is taken from one seeded RNG in a fixed order; background noise is derived
  from request ids by hashing. Timing may drift; the stream does not.
- **Every run is a snapshot**: `data/scenarios/<id>/<run>/` holds the manifest
  (absolute timestamps, journal entries), the logs, the journal, and the final
  routing state. Ground truth is relative; manifests are absolute; the join is
  the deploy id.
- **The reproducibility check is a script, not a claim**: `just s1-check`
  compares two runs against the pre-registered tolerances and exits non-zero
  on failure.
- **Every scenario directory carries a README with its measured acceptance**,
  so the corpus documents its own validity.

`bench/` (Phase 10) consumes the same contract: conditions × scenarios ×
repeats, every run file committed, tables generated from them and never
hand-edited.

## Alternatives considered

- **Hard-code the demo.** Rejected: unfalsifiable, single-use, and it makes the
  benchmark a story rather than a measurement.
- **Ground truth derived after the fact from what the agent found.** Rejected:
  that is scoring the answer key against the answer.
- **Non-deterministic traffic ("realistic randomness").** Rejected: run-to-run
  variance in the *incident* would be indistinguishable from variance in the
  *agent*, and the benchmark measures the agent.

## Consequences

- Slightly more structure now: a schema, a validator, run manifests.
- Phase 1's acceptance came out stronger than the bar required: two
  default-timeline runs produced byte-identical error curves because both
  completed exactly 4,762 checkouts before the fault. Determinism to the
  request, not merely within tolerance.
- After the hackathon, the corpus is where new scenarios, other people's
  agents, and any future evaluation work would land.

## Reversal conditions

None. Removing this removes the project's measurement story.
