# ADR-006 — Novelty detection via template mining, and no embeddings

**Status:** Accepted · **Date:** 2026-08-28 (expanded at Phase 4, when the detector was built)

## Context

The incident signal is "what is new": a template never seen before, or one
whose rate jumped. Logs are machine-templated text — a few dozen shapes
repeated hundreds of thousands of times — so lexical structure is nearly
lossless, and "new" is a question about template identity and time, not
about meaning.

## Decision

- **Template identity by simplified Drain** (He et al., ICWS 2017): mask
  variable parts (numbers, UUIDs, hex, timestamps, currency codes → `<*>`),
  tokenise, route through a fixed-depth tree keyed on token count and leading
  tokens, and merge into the best cluster at the leaf when at least half the
  positions agree. Two rules added on evidence from this system's own logs:
  the **log level is the leading token** (so `INFO request completed` and
  `ERROR request failed` can never merge into `request <*>`), and a merge needs
  **at least two agreeing positions** (two-token messages sharing one token
  are a coincidence, not a template).
- **Novelty score**, a pure function with unit tests:
  `1.0` if the template first appeared inside the window; else
  `min(1, log2(rate_window / rate_baseline) / 6)` if its rate jumped (64× → 1.0);
  else `0`; ×1.25 if the dominant level is ERROR, capped at 1.0. Weights and
  thresholds in `spyglass.toml`.
- **Two guards that keep the score honest**: a template first seen within
  `warmup_secs` of the engine's earliest event is pre-existing vocabulary
  (the engine started mid-stream; it cannot call that "new"); and the
  baseline is clipped to real history — with fewer than `min_baseline_secs`
  of it, burst novelty is *undetermined* (`0`, reason
  `insufficient_baseline`), never inflated. Without the second guard every
  steady template scored 0.84–1.0 in a quiet window. The false-positive
  check exists for exactly this.
- **Ranking within the tool** is documented, not learned: novelty desc,
  severity desc, has_stack desc, first_seen asc (the earliest novel thing is
  the likeliest origin), count desc, template_id asc.
- **No vector search.** Nothing here embeds anything.

## Alternatives considered

- **Embedding similarity.** Rejected for v0: heavier, slower, rankings that
  cannot be explained factor by factor, and templated logs make the lexical
  route nearly lossless. Semantically-novel-but-lexically-similar messages
  can be missed; accepted and listed under Failure Modes.
- **Raw grep.** Rejected: no notion of "new".
- **Masking alone (Phase 3's stepping stone).** It produced the same 22
  templates on this corpus; Drain's merge earned its place on the unit tests
  (`user alice logged in` / `user bob logged in`) rather than on S1, whose
  variables are all maskable. Recorded so nobody mistakes the corpus for the
  algorithm.

## Consequences

- Phase 4 acceptance: on S1 the seeded ERROR template ranks #1 with
  `first_seen` 0.1 s after the deploy; the incident window returns exactly
  the five templates that first appeared in it; three quiet windows return
  nothing above threshold.
- The decoy — a benign INFO template new in the same deploy — scores 1.0
  too and ranks #5 behind the four ERROR templates. That is correct
  behaviour for a *novelty* tool; separating "new and harmful" from "new and
  harmless" is the ranker's job (ADR-008), with severity as its own factor.
- Residual over-merge on very short messages is bounded by the
  two-agreeing-positions rule; a length-dependent threshold is the next
  refinement if a corpus needs it.

## Reversal conditions

If a scenario's seeded fault reuses an existing template verbatim (S3 is
built that way), first-seen novelty is silent by design and burst novelty
must carry it — measured in Phase 10. If burst also misses, that is reported,
not hidden.
