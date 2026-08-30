# ADR-005 — Bounded evidence, never unrestricted telemetry

**Status:** Accepted · **Date:** 2026-08-28 (expanded at Phase 3)

## Context

The failure mode being engineered against is context flooding: an agent that
pages raw telemetry until its context is full of repetition and its answer is
wrong (ITBench-AA's over-investigation). Phase 2 measured what that costs on
the *easy* scenario: 57 KB of raw log into context, 198k input tokens, for an
answer whose evidence fits in a few hundred bytes.

## Decision

Every tool enforces item and byte bounds **in the engine**, and there is no
"give me everything" tool:

- `max_items` (20) per response; `search_logs` may ask for up to 50 templates.
- `max_bytes_per_item` (2 KB): an item over the cap has its longest string
  halved until it fits, and says `…[capped]`.
- One excerpt per template (600 B), deduplicated by construction — the unit
  of a search result is a *template*, never a raw page.
- `meta.bounds` on every response reports `items_returned`,
  `items_available`, and `truncated`, so the agent knows what it did not see
  and can ask a narrower question.
- Pathological needs go through `get_evidence(eid)` one record at a time.

Bounds live in `spyglass.toml`, not in code, so raising them is a
measurement rather than a rewrite.

## Alternatives considered

- **Trust the model to ask for less.** Rejected: the baseline condition
  exists precisely to show what that costs, and Phase 2 showed it.
- **Bound by prompt** ("please be brief"). Rejected: a prompt cannot bound a
  context window; only the thing producing the bytes can.

## Consequences

- The agent must iterate through bounded views. That is the point: it converts
  reading comprehension into structured reasoning.
- Bounds too tight would starve the agent. They are config; both settings get
  measured if it comes to that.
- The baseline's raw tools also have caps (1000 lines), because real `tail`
  and `grep` do — but they are caps on raw lines, not on evidence. The
  difference between the two conditions is *shaping*, not *hiding*
  (`bench/conditions/README.md`).

## Reversal conditions

Caps are config; raising them is a measurement, not a rewrite.
