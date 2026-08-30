# ADR-013 — No custom frontend; the harness UI and the terminal are the surface

**Status:** Accepted (recorded Phase 0 in the README; confronted and expanded
Phase 11, 2026-08-29)
**Related:** ADR-003 (MCP as the tool boundary), ADR-009 (the ledger), ADR-011
(the gate)

## Context

Three days of build time, and a demo that has to show — not narrate — an
agent investigating, an experiment, a human approval, an engine-judged
close, and a re-checkable ledger. TrueForge ships a session UI that already
renders every tool call with its arguments and result, sub-agent threads,
sandbox activity and the approval card for a gated tool. The question Phase
11 actually confronted: when the filming runbook was written, was there any
frame the demo needed that the harness UI plus a terminal could not show?

## Decision

The harness UI is the UI. The additions are terminals, not web pages:

- `just watch` — the error-rate dashboard and the alert (the "stakes"
  frame, 0:00–0:10);
- the runner (`scripts/investigate.py`) — per-turn tool calls and token
  totals, and **the gate rendered with every cited evidence id resolved to
  the ledger line that produced it** (the control-and-safety frame,
  2:00–2:25) — this is the one place a custom surface was considered, and a
  terminal rendering turned out to be *more* legible than a card would be;
- `just ledger-check` — the re-check verdict, digests matching on screen;
- the README's generated results table — the closing numbers.

Nothing in the repository serves HTML.

## Alternatives considered

- **A custom incident-timeline UI** (evidence items on a time axis, deploys
  as markers, the causal check inline). Deferred: a day of work with no
  thesis value — the thesis is about what reaches the model, not about what
  reaches the human — and it competes with, rather than showcases, the
  sponsor's surface. Every frame it would add is a frame the session page
  already shows as a rendered tool result.
- **A richer approval card** in the harness (eids resolved inline). Not
  possible from outside the harness; the runner's terminal rendering covers
  it, and the README says why the human should be approving *evidence*.

## Consequences

- The demo is filmed off the session page and three terminals
  (`docs/demo.md` → Filming-day runbook); the session page is the star and
  must hold ≥ 60 % of the frame.
- Evidence responses are designed to be *read on the session page*: compact
  items, a `headline` per item, `meta.engine_latency_ms` and `meta.eids` at
  the top level. That constraint shaped the tool payloads (Phase 5's compact
  changepoints, Phase 7's ≤ 8 kB bundle) more than any frontend would have.
- The results table is generated Markdown, not a chart: a judge can diff it
  against the run files.

## Reversal conditions

Post-hackathon, the first UI worth building is an evidence-timeline view
over the ledger — the ledger already carries every eid with its window,
digest and latency, so the view is a reader, not a new data path. It becomes
worth building when someone other than the author has to read an
investigation after the fact.
