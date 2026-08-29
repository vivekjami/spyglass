# ADR-004 — Deterministic retrieval where possible

**Status:** Accepted · **Date:** 2026-08-28 (expanded at Phase 3, when the engine was built)

## Context

The ledger's promise is that every citation in a postmortem can be
re-executed later and found to say the same thing. The benchmark's promise is
that an ablation compares like with like. Both need the same property: the
same query over the same data returns the same bytes.

Retrieval is easy to make *almost* deterministic and hard to make exactly so —
hash-map iteration order, ties broken by whatever the allocator did, windows
that silently mean "now".

## Decision

Every read tool is deterministic on frozen data, by construction:

- **Explicit, resolved windows.** A tool called without a window gets one —
  `[watermark − 15 min, watermark]` — and the *resolved* window is what goes
  into the ledger, so the replay asks the exact question that was asked.
- **Stable sort keys with explicit tie-breaks.** `search_logs`: score desc,
  count desc, first_seen asc, template_id asc. `error_delta`: delta desc,
  group asc. `deploy_events`: ts asc, journal `n` asc.
- **Canonical digests.** The result digest is SHA-256 over canonical JSON
  (sorted keys) with evidence ids stripped, because ids are assigned per
  investigation and must not perturb the digest.
- **Temporal tools say so.** `freshness_watermark` is inherently about now;
  its ledger entry is marked `deterministic: false` and the re-check skips it.
  `get_evidence` is deterministic but session-scoped (ids live in one
  investigation), so the re-check skips it too and says why.

## Alternatives considered

- **"Roughly the same" retrieval.** Rejected: kills auditability and makes
  ablations noisy in ways that look like signal.
- **Bit-identical agent trajectories.** Not attempted: the model is
  nondeterministic. The claim is re-checkable *evidence*, not replayable
  reasoning.

## Consequences

- Verified in Phase 3: a ledger of six entries re-executed against the live
  engine — five deterministic entries matched digest-for-digest, one temporal
  entry skipped. `scripts/ledger-check.py` is the test; `just ledger-check`
  runs it.
- The store keeps growing while the engine runs, so a window that is not yet
  fully ingested when first queried could yield a different digest on replay.
  Late arrival is bounded by ingest lag (single-digit seconds), and the SOP
  requires the watermark check before concluding; the re-check is run after
  the investigation, not during it.

## Reversal conditions

None — this is a correctness property.

## Addendum (Phase 7): the live edge

A window that ends at the *newest* ingested timestamp is not frozen: the
tailer reads the files one after another, so a line written to an
already-read file milliseconds before another file's newest line is inside
the window and not yet in the store; the replay sees it and the digest
moves (Phase 7 F6 — one event, 227 bytes; Phase 3's single mismatch was
very likely the same). Windows now resolve their end at the **safe
watermark** — the newest timestamp every active source has been read past
— and a requested end beyond it is clamped and recorded as clamped. Idle
sources (more than 5 s behind the newest) do not hold it back.
