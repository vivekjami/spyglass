# ADR-001 — An evidence layer exists

**Status:** Accepted · **Date:** 2026-08-28 (recorded at project start)

## Context

Published evaluations show agent root-cause analysis failing on raw telemetry.
ITBench-AA (May 2026) measured every frontier model below 50% on Kubernetes
incident RCA, with **over-investigation** as the documented failure mode, and
found that *longer trajectories did not improve accuracy*. ITBench
(arXiv:2502.05352) measured ~13.8% SRE scenario resolution.

The last point is decisive. If the bottleneck were reasoning effort, more steps
would help. They don't. That points upstream of the model.

## Decision

Insert a dedicated **evidence plane** between production telemetry and the
reasoning model: ingest, index, mine templates, score novelty, detect
changepoints, rank, and assemble bounded evidence bundles — *before* the model
sees anything. The agent never receives unrestricted telemetry access.

## Alternatives considered

- **Better prompts over raw tools.** Rejected: prompts cannot bound a context
  window and cannot rank 184,000 events. "Please be brief" is not an
  enforcement mechanism.
- **Bigger context windows.** Rejected: cost scales with the garbage as well as
  the signal, and ITBench-AA already showed longer trajectories don't help.
- **Fine-tune a model on incidents.** Rejected: off-thesis (the claim is about
  evidence, not weights), data-hungry, and unexplainable.

## Consequences

- An extra service to build, run, and operate.
- A clean seam to benchmark: the same model can be run with and without it.
- The project lives or dies on a measurable comparison rather than a demo.

## Reversal conditions

If the benchmark shows no material gain over the baseline under identical
conditions, the thesis is falsified — and the writeup says so plainly, with all
runs committed. See Engineering Principle 12.
