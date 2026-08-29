# ADR-012 — The baseline uses the same model, and only the evidence interface varies

**Status:** Accepted · **Date:** 2026-08-28 (recorded in the README at design time; written in full at Phase 10, when the benchmark ran)

## Context

The project's claim is falsifiable only if it is *about evidence*: shaped,
bounded, evidence-id-stamped telemetry makes the same model faster,
cheaper and more accurate at incident RCA than raw telemetry tools. Every
other difference between two agents — the model, the harness settings, the
information they can reach, the action they can take, the alert that wakes
them — is a confound. A benchmark that varies any of them measures something
else and calls it Spyglass.

Phase 2 built the control group before the treatment existed so the number
to beat would be honest (the baseline *solved* S1, in 39.5 s and 198k input
tokens). Phase 10 ran the comparison.

## Decision

1. **Same model.** Every condition's manifest names `$MODEL_A`, resolved to
   the same catalog entry by `scripts/tf-setup.py`. The run file records
   which model answered.
2. **Same harness settings.** `iteration_limit`, sandbox, sub-agents,
   `preload: true`, and — critically — `context_management.compaction` and
   `large_tool_response` **off** in every condition
   ([ADR-016](ADR-016-harness-context-management-pinning.md)): the harness
   does no shaping of its own, so "raw" is raw.
3. **Same information access.** The baseline's tools read the same files and
   endpoints the engine reads (`bench/conditions/README.md` maps them one to
   one), with generous limits that report their own truncation. When the
   engine gained a capability the raw tools lacked, the baseline got the raw
   counterpart the same phase: `http_request` for the causal replay (Phase
   8); the unobserved `fraudcheck` dependency in both topologies (Phase 10).
   What the baseline does not get is ranking, novelty, dedup, bounds and
   evidence ids — that is the treatment.
4. **Same action path.** One deployer server, one `propose_rollback` →
   gated `rollback(proposal_id, …)` flow, one journal, one gate policy —
   in the benchmark, auto-approve, so a wrong proposal executes and is
   scored as a wrong action rather than hidden behind a simulated human's
   judgment.
5. **Same alert.** The scenario's `alert` text from its ground truth, for
   every condition; the runner never says which scenario it is.
6. **Same scorer.** Terminal state and verdict block are scored identically;
   evidence precision/recall exist only where evidence ids exist, and the
   report says "n/a (no eids)" for the baseline rather than inventing a
   proxy. That the baseline's claims are not re-checkable is itself a result.
7. **A competent baseline prompt, not a strawman.** A method, the tool list,
   "be thorough", the data-not-instructions rule, the same closing verdict
   block, and the same guidance about non-deploy causes and refusal — no
   hints about which log lines matter. If the baseline wins a cell, the
   table says so.

## Alternatives considered

- **A stronger model for Spyglass.** Rejected: confounds the claim with
  model choice; the generalization experiment (Model B) exists for the
  opposite reason — to show the engine is model-agnostic.
- **No control group; report Spyglass's numbers alone.** Rejected:
  uninterpretable. "12 tool calls" means nothing without "19 with raw tools".
- **A weakened baseline (no `http_request`, no deploy journal, a thinner
  prompt).** Rejected: a strawman invalidates the whole result and would be
  found by the first reader who opened `bench/conditions/`.
- **An LLM judge for RCA correctness.** Rejected for v0: a mechanically
  parsed verdict block plus pre-registered accepted values is auditable and
  free; a judge adds a model's opinion to a measurement of models. Revisit
  if verdict parsing proves too brittle.
- **Different repeats per condition.** Rejected: n is the same everywhere
  (3), and every run — including invalid ones — is committed.

## Consequences

- The baseline had to be built first (Phase 2) and maintained in lockstep
  (Phases 8, 10): every engine capability with an information component
  costs a raw counterpart.
- The comparison on S1 is about *cost and auditability*, not correctness —
  the baseline finds S1. The accuracy story lives in S2, S3 and S6, where
  the smoking gun is absent, not a deploy, or not in the telemetry.
- The benchmark inherits the harness's approval semantics: the gate is real
  in the demo and simulated in the matrix; the report states which.
- Metrics that depend on evidence ids are one-sided by construction and
  labelled as such.

## Reversal conditions

If the harness ever ships shaping that cannot be disabled, ADR-016's pinning
fails and this decision has to be re-argued with the harness as part of the
treatment. If a future condition changes the model, it is a different
experiment (the generalization section), not this one.
