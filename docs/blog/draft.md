# Spyglass: making the evidence better instead of the model smarter

*Hackathon blog draft — grown incrementally from ADRs and build notes. Engineering
notes, not marketing copy. Sections marked ⏳ are filled in by the phase that
produces them; nothing below claims a result that has not been measured.*

---

## The hypothesis

AI agents are measurably bad at production incident investigation, and the
published reason is not "the model can't reason." On ITBench-AA (May 2026),
every frontier model scored under 50% on Kubernetes root-cause tasks, the
failure mode was *over-investigation*, and — the detail that shaped this whole
project — **longer trajectories did not improve accuracy**. If more steps and
more tokens made agents better at this, the fix would be patience and budget.
They don't. So the problem is upstream of the reasoning.

Spyglass is one bet: incident investigation is an *evidence* problem before it
is a reasoning problem. Put a deterministic evidence plane between the
telemetry and the model — template mining, novelty, changepoints, ranking,
bounded bundles — and the *same* model should get faster, cheaper, and more
accurate. Hold the model constant, vary only the evidence interface, and
measure. If the prediction fails, say so.

## The architecture in one paragraph

A Rust engine ingests logs, metrics, and deploy events and serves the agent
bounded, ranked, evidence-id-stamped facts over MCP. TrueForge runs the agent
loop, subagents, sandbox, and approval gate — none of that is rebuilt here. A
suspected root cause is not acted on from correlation alone: the captured
failing request is replayed against the suspected-bad and known-good versions,
turning "deployed two minutes before the errors" into "fails 19/20 on v2, 0/20
on v1." The one mutating action, `rollback`, sits behind a human approval gate,
is idempotent, and is followed by a verification loop. Every consequential tool
result lands in an append-only ledger; the postmortem cites ledger entries.

## Things that broke (and what they taught)

This section is the honest part. Written as it happens.

### Phase 0 — the harness is nine days old

- **Node 22, not 20.** TrueForge requires ≥22.14; Ubuntu ships 20. No root on
  the build machine, so: official tarball, checksum-verified, into `~/.local`.
  First lesson of the hackathon was a version number.
- **The sandbox does not need Daytona.** The docs say Daytona is the only
  provider. The startup log said otherwise: a local fallback on
  `@anthropic-ai/sandbox-runtime` exists if `bwrap`, `socat`, and `rg` are on
  the host. Built socat from source into `~/.local`; sandbox came up. That
  looked like the biggest risk in the plan evaporating.
- **…but the sandbox cannot reach the Compose network.** Tested directly
  against the runtime, no model in the loop: it removes the network namespace,
  sets `NO_PROXY` to every private range, and the harness's proxy allowlist is
  a hard-coded constant (pypi, github). The proxy *will* forward to a private
  IP when allowlisted and the client is told to use it — so the mechanism is
  real — but there is no supported knob. The causal replay moves into a
  bounded MCP tool on the evidence plane, which *is* on the Compose network.
  The controlled experiment survives; its executor changes. Three failed
  attempts at testing this through an LLM on a rate-limited key preceded the
  five-minute direct test that settled it. Test the layer you are asking about.
- **The harness shapes tool responses by default.** `large_tool_response`
  and `compaction` default to on. Left alone, the *baseline* condition — the
  one that exists to show what raw telemetry costs — would have received
  harness-shaped telemetry. The control group getting a weak dose of the
  treatment. Pinned off in every condition (ADR-016). This is the finding I'd
  most want another team to hear.
- **Subagents are dynamic.** No static definitions, no per-subagent budgets.
  The three analysts became text the lead generates; real budgets moved to
  `iteration_limit` and engine-side rate limiting.
- **A free-tier key is 20 requests per day.** Four model calls per tool use
  under deferred loading; 20–50 per investigation; 27–54 investigations in the
  benchmark. And the harness does not retry a 429 — the turn just dies. Not a
  slow key, an unusable one. Paid tier, same model.
- **Turn-level token usage is the last call only.** A turn with one trivial
  sub-agent reported 1340/30 tokens; summing across its three model calls
  gave 2745/197. The benchmark runner sums across every thread. A runner that
  read the turn-level number would have under-counted by 2× on a trivial
  fan-out.

### Phase 1 — a deterministic incident

- **A deploy is a file write.** Both payments versions run always-on; `orders`
  reads `current.json` per request. First seeded error 0.6 s after the deploy
  journal entry. Rollback will be as fast.
- **Reproducibility came out stronger than the bar.** Two runs on the default
  timeline produced *byte-identical* error curves — both completed exactly
  4,762 checkouts before the fault, so the same seeded request hit `v2` first.
  The stream is a pure function of the seed; with a fixed timeline, so is the
  incident.
- **Port 8080 was taken**, by something root-owned. Ports became config. A
  judge's machine will have the same problem.
- **httpx logs every call at INFO.** A second copy of every request in the
  logs, contributing nothing. Silenced — inflated volume would have flattered
  the baseline's token count without adding information.
- **The decoys are real.** 553 benign new-in-v2 INFO lines against 137 seeded
  ERROR lines; a benign deploy six minutes earlier that changed nothing; WARN
  chatter before and after; injection-styled user-agents captured verbatim. A
  novelty ranker that keys on "new" alone will rank the decoy first. That is
  the point.

### Phase 2 — the control group

- **`pgrep -f` matches the shell that runs it.** `just mcp-up` reported the
  MCP servers as already running and started nothing; its liveness check
  matched the recipe's own command line. The Phase 0 `pkill -f trueforge`
  lesson, learned twice. Liveness is now by listening port, everywhere.
- **Gemini rejects `Option<Vec<T>>`.** schemars emits `anyOf: [array, null]`
  for an optional list; Gemini's function-declaration validator wants
  `items` at the top level and fails the whole request — every tool, not
  just the offending one — before the first model call. A `Vec<T>` with a
  serde default is what an "optional list" has to look like. The failed run
  is committed in `bench/results/` like any other: a harness error is a
  result too.
- **The baseline's tools are honest tools.** A `tail`, a `grep`, a `curl
  /metrics`, a `cat journal` — with generous caps, and every truncated
  response starting with *"2 of 11,812 matching lines"* so the agent can
  page. If the control group finds the root cause fast, that is a finding,
  and it gets reported.

### Phase 3 onward ⏳

## The benchmark ⏳

*Populated by `bench/report.py` from committed run files. Method: same model,
same harness, same information access, same action path; baseline vs.
Spyglass vs. no-novelty ablation; S1–S3 × 3 repeats; n=3 is a hackathon
budget, not a study, and no significance is claimed.*

## Results, including anything negative ⏳

## Limitations

n=3 · self-authored scenarios (mitigated by pre-registered ground truth and
committed raw runs, not eliminated) · one incident domain · the causal replay
runs on the evidence plane rather than in the harness sandbox (see Phase 0) ·
⏳ *more as they are found.*

## What would be built next

Agent-session forensics: point the same evidence plane at TrueForge session
logs to diagnose *other agents'* loops, token burns, and failure patterns.
Same engine, different telemetry.
