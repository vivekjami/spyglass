# Spyglass documentation

The root [`README.md`](../README.md) is the source of truth for *what* Spyglass is
and *how* it is built. This folder holds the material that would bloat it, plus
the running record of decisions and findings.

## Reading order

| Doc | Read it for |
|---|---|
| [`motivation.md`](motivation.md) | **Start here.** Why this project exists, what problem it attacks, and what would prove it wrong. |
| [`architecture.md`](architecture.md) | How the evidence plane is built, component by component. |
| [`phase0-findings.md`](phase0-findings.md) | What we actually verified about the TrueForge harness, and where reality diverged from the spec. |
| [`adr/`](adr/) | Individual decision records — context, alternatives, why rejected, reversal conditions. |
| [`safety.md`](safety.md) | The safety model: read/write separation, injection defence, approval gate. |
| [`benchmark.md`](benchmark.md) | Benchmark methodology and results (generated, never hand-edited). |
| [`demo.md`](demo.md) | Shot list and per-segment commands. |

## Conventions

These are load-bearing, not stylistic:

- **`[MEASURE AFTER IMPLEMENTATION]`** marks a number that does not exist yet.
  It is never replaced by an estimate, only by a measured value.
- **"Decision pending implementation experiment"** marks a choice deliberately
  left open until code forces it.
- **"Future / optional"** marks speculation, so it cannot be mistaken for a plan.
- ADRs are written *when the decision is made*, not backfilled before submission.
- If code and docs disagree, one of them is fixed in the same PR.
