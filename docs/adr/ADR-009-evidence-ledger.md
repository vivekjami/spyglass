# ADR-009 — An evidence ledger, not just an RCA

**Status:** Accepted · **Date:** 2026-08-28 (expanded at Phase 3)

## Context

An LLM-written RCA is a persuasive essay: fluent, plausible, and
unfalsifiable. Enterprises asked to trust an agent's incident conclusions
need something they can check after the fact — and engineers debugging a wrong
conclusion need to know whether the failure was retrieval (bad evidence
served), ranking (good evidence buried), or reasoning (good evidence, bad
synthesis). Those have different fixes and are indistinguishable from prose.

## Decision

The engine writes an append-only JSONL ledger per investigation, keyed by
the MCP session (`ledger/<session>.jsonl`), and issues evidence ids
(`E1…En`) that are the currency of the whole system:

- **Every tool response** appends one entry: `n`, `ts`, `tool`, the
  *resolved* `args`, `args_hash`, `result_digest`, the `eids` issued, a
  one-line `summary`, `latency_ms`, `deterministic`.
- **Every evidence item** gets an eid at response time and is persisted to
  `ledger/<session>.evidence.jsonl`; `get_evidence(eid)` dereferences it.
- **The SOP makes eids mandatory**: a claim without one is an unsupported
  claim. The rollback tool records `justification_eids` in the deployer
  journal, so the action cites its evidence too.
- **The benchmark runner** joins the RCA's cited eids against the ledger's
  issued eids (`eids_cited_valid`) — the raw material for evidence precision
  and recall — and runs the digest re-check after every investigation.

## Alternatives considered

- **Rely on TrueForge's session events.** Kept as a complement, rejected as
  primary: harness events record the *conversation*; the ledger records
  *evidence semantics* — resolved args, digests, ids — and survives
  independent of harness internals.
- **Ledger written by a client wrapper.** The engine writes its own entries
  (it knows the resolved args and the digest); client-side entries for
  actions and sandbox results are a Phase 9 addition.

## Consequences

- A thin cost per call (one JSON line) for a large trust gain.
- The postmortem becomes a rendering of the ledger plus prose, not a parallel
  account that can drift from what happened.
- Ids are per investigation, so digests strip them (ADR-004) and
  `get_evidence` cannot be replayed from a different session — the re-check
  says so rather than pretending.

## Reversal conditions

None — removing it removes the project's accountability story.
