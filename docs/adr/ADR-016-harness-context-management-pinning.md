# ADR-016 — Pin the harness's context-management flags in every benchmark condition

**Status:** Accepted (Phase 0, 2026-08-28)
**Supersedes / amends:** extends ADR-012 (the baseline uses the same model)

## Context

ADR-012 defines the experiment: baseline and treatment are identical in model,
harness, incident, information access, and action path, and differ **only** in
the evidence interface. Any hidden asymmetry — or hidden *symmetry* that does
work on the control group's behalf — invalidates the result.

Phase 0 inspection of the TrueForge agent manifest turned up two flags that were
not on anyone's radar:

```json
"config": {
  "context_management": {
    "compaction":          {"enabled": true, "trigger": {"type": "input_tokens", "value": <int>}},
    "large_tool_response": {"enabled": true}
  }
}
```

Both default to **`true`**. `large_tool_response` means the harness performs its
own shaping of oversized tool results before they reach the model. That is a
weaker, generic version of exactly the job Spyglass exists to do.

Left at defaults, the BASELINE condition — the one whose whole purpose is to
show what unshaped telemetry costs — would quietly receive *shaped* telemetry.
Three problems follow:

1. The measured Spyglass gain would be **understated**, because the control
   group is partly treated.
2. "Raw telemetry tools" in the writeup would be **false**: the raw tool results
   are being post-processed before the model sees them.
3. The comparison becomes **indefensible** the moment a judge or reader asks what
   the harness was doing to the baseline's tool output.

Compaction has the same character: it evicts and summarises context mid-run.
Since "context-window pressure" is one of the pathologies Spyglass claims to
attack, letting the harness mitigate it differently across conditions — or at
all, unrecorded — confounds the token and accuracy metrics.

## Decision

**Disable both `compaction` and `large_tool_response` in every benchmark
condition — baseline, spyglass, and all ablations — and state the setting
explicitly in each condition file rather than relying on defaults.**

The flags are written out in full in `bench/conditions/*.json` even where the
value equals the default, so that a reader of any single condition file can see
what the harness was doing without cross-referencing TrueForge's version.

## Alternatives considered

- **Leave defaults on for both conditions.** Rejected: symmetric, but it makes
  "raw tools" untrue and puts a confound *inside* the dependent variable
  (tokens). It also means a TrueForge version bump could silently change the
  benchmark.
- **Leave them on for baseline only, off for Spyglass.** Rejected outright —
  that is the strawman-baseline failure ADR-012 exists to prevent, in reverse.
- **Leave them on for Spyglass only.** Rejected: double-shaping would inflate
  the treatment's numbers and make the engine's contribution unattributable.
- **Report both settings as an extra 2×2.** Rejected for the hackathon: it
  doubles run count against a floor we have not yet met. Recorded as Future /
  optional — it is a genuinely interesting question (*does an evidence plane
  still help when the harness already shapes?*) and the right follow-up.

## Consequences

- The baseline is honestly raw: its context pressure is real, its token blowout
  is real, and its over-investigation is unmitigated. That is the phenomenon
  under study, not an artifact we introduced.
- Baseline runs may hit the context window and fail. **That is a result**, not a
  bug — it is recorded as a failure mode with its run file committed, per
  Engineering Principle 12.
- `config.iteration_limit` must be pinned identically for the same reason, and
  is listed in the condition files alongside these two.
- `docs/benchmark.md` states the pinned values as part of the methodology.

## Amendment (same day) — deferred tool loading

Live traces showed the agent calling the harness built-ins `list_tools` and
`get_tool_info` before every first use of an MCP tool. This is TrueForge's
deferred tool loading, and it adds 1–2 calls per tool touched — directly onto
benchmark metric 5 (tool calls) and onto tokens, unevenly across conditions
(5 raw tools vs 10 shaped ones). Same class of confound as above, same
treatment: **`preload: true` is pinned on every MCP server in every condition
file.** The count then measures the agent's investigation, not the harness's
tool discovery.

## Reversal conditions

If disabling compaction causes baseline runs to fail so early that they produce
no usable trajectory at all, the comparison loses its foil. In that case: turn
compaction back on **for both conditions**, pin the identical trigger value,
record the change here, and report the numbers as measured under that setting —
never with the two conditions differing.
