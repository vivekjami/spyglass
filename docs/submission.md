# Submission — The Agent Harness Hackathon

Everything the submission form asks for, in one place, so the last hour is
copy-and-paste. Deadline: **Sun Aug 30 2026, 20:00 London = 00:30 IST Mon**;
internal target 22:00 IST. Form: <https://forms.gle/PxGLsWW1HPyroQ5u9>.

| Form field | Value |
|---|---|
| Project | **Spyglass** — evidence engineering for AI-powered incident investigation |
| Team | Vivek Jami (solo) |
| Public repository | <https://github.com/vivekjami/spyglass> |
| Demo video (≈3:00) | _paste the link after recording — see [`demo.md`](demo.md) → Filming-day runbook_ |
| Write-up | the section below (also `docs/blog/draft.md` for the long form) |
| Qodo Code Review Evidence | `README.md` → [Qodo Code Review Evidence](../README.md#qodo-code-review-evidence) |
| TrueForge capabilities demonstrated | MCP tool access (4 servers, 21 tools), the human approval gate on `rollback`, the sandbox (agent-paced verification), dynamic sub-agents, programmatic sessions over the REST API |

## Write-up (paste into the form)

**What the agent does.** Spyglass is an on-call incident investigator built
on TrueForge. An alert fires on a small e-commerce system (gateway → orders →
payments, with postgres, redis and an unobserved fraud vendor behind them).
The agent finds the root cause, tests it before acting, acts only through a
human-approved gate, lets the engine verify recovery, and writes a
postmortem in which every claim cites an evidence id. Not every incident has
a deploy behind it and not every cause is visible in telemetry, so
*report-only* and *refuse-and-say-what-would-decide-it* are first-class
outcomes, scored as successes when they are the right call.

**The idea.** Incident investigation is an evidence problem before it is a
reasoning problem (ITBench-AA: frontier models under 50 %, longer
trajectories did not help). So the build is not a smarter agent but an
*evidence plane*: a Rust engine that ingests logs, metrics and deploy events
and serves the agent bounded, ranked, deduplicated facts over MCP — novel log
templates (Drain, level-keyed), changepoints with the nearest deploy
annotated, a ranked bundle that turns thousands of events into a handful of
items with score factors and relationships, and a **causal check**: the
captured failing request replayed N times against each always-on version, so
"deployed two minutes before the errors" becomes "0/20 on v1, 20/20 on v2".
Every response carries evidence ids, canonical digests and engine latency;
an append-only ledger makes the investigation re-checkable after the fact.

**TrueForge integration.** The three benchmark conditions are three TrueForge
agent manifests (`bench/conditions/`), registered by name over the REST API
and differing only in which MCP server supplies the read tools — the same
model, harness, incident and action path. The evidence engine, the
raw-telemetry baseline tools, the ablation engine and the deployer are four
`rmcp` streamable-HTTP MCP servers. The one mutating tool, `rollback`, sits
behind `require_approval_for_tools`; the agent can only *propose* — the
deployer mints the proposal id, snapshots the live version and stamps an
expiry, and execution is idempotent, TOCTOU-checked and refused if the
restatement differs. Recovery is judged by the engine (`verify_recovery`),
never declared by the model; the agent paces its checks with the sandbox.
`context_management` is pinned off in every condition so the harness does not
shape the control group's evidence (ADR-016). Sessions, turns, approvals and
cross-thread token accounting are driven programmatically for the benchmark.

**What was measured.** Same model, same harness, four scenarios (a deploy
regression with a smoking gun; a config-only release that becomes a latency
cascade; a redis full with no change event; an unobserved vendor degrading
with a benign deploy as bait), three conditions, three repeats, every run
committed — including the ones that went wrong.

<!-- numbers:begin -->
Results (36/36 valid runs; means of n = 3; `docs/benchmark.md` has the
ranges and the file behind every number): on the three scenarios whose
cause is in the telemetry, **every condition was 9/9** — the same model
with raw tools found the deploy regression, the config-only release and
the redis fill too. The evidence plane changed the shape of the
investigation and, on one scenario, the bill: on S3 Spyglass used 8.7 tool
calls and 139k input tokens against the baseline's 14 and 210k, with every
claim cited; on S1 and S2 it cost slightly more, all of it in the causal
check and the engine-judged verification the baseline does not have. The
refusal scenario S6 is the negative result and the most useful one: the
no-novelty ablation refused correctly 3/3, Spyglass 1/3, the baseline 0/3
— the novelty templates gave the model material to blame a benign deploy.
More evidence made the agent act; that finding, and the two engine gaps it
exposed (verification judges only the 5xx share; no mechanical evidence
floor on proposals), are recorded rather than patched over, because the
benchmark exists to be able to produce them.
<!-- numbers:end -->

**Process.** Eleven stacked pull requests, one per phase, each with a
findings document; the automated review's fifteen findings are itemised in
the README with what was fixed and what was dismissed and why; a CI
workflow keeps the generated benchmark tables consistent with the committed
run files.

## Before submitting — the operator's list

Only these need a human; everything else is in the repository.

1. **Authorize Qodo Merge on the repository** (GitHub → Marketplace → Qodo
   Merge, or <https://github.com/apps/qodo-merge-pro>), then comment
   `/review` on PR #10 and PR #11 (Qodo also reviews on open by default).
   Paste the review links into the README's *Qodo status* line.
2. **Merge in stack order** so `main` ends up with everything: #11 → #10 →
   then the chain below it is already merged into `phase-1-incident-environment`;
   open one PR `phase-1-incident-environment → main` (or merge `phase-11-demo`
   into main directly once #10/#11 are in) — the README's *Qodo* section and
   `docs/progress.md` link the trail either way.
3. **Record the video** per [`demo.md`](demo.md) → Filming-day runbook (two
   takes; voiceover separate; ≤ 3:00), upload, paste the link above.
4. **Submit the form** with the table at the top of this file.
