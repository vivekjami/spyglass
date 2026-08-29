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

### Phase 3 — the ugly loop

- **A stale process on the engine's port.** `just mcp-up` said "engine:
  already on :8791" — it was yesterday's Phase 0 probe, still running, and the
  harness dutifully listed `probe_ping` as the evidence engine's tools.
  Port-based liveness is right; it just cannot tell you *which* process owns
  the port. Check the tool list, not the port.
- **Create and update take different bodies.** `POST /agents` wants
  `{name, manifest}`; `PUT /agents/{id}` rejects `name`. The setup script had
  been dying on the update for an hour without anyone noticing, because the
  first condition already existed and the second was never reached. The
  `spyglass` agent did not exist until `just demo` failed on it.
- **The empty result was the right result.** The first `search_logs` for the
  seeded error returned nothing — because the default window is the last 15
  minutes of ingested data, and the incident was an hour old. Windowed
  evidence means "not now" is an answer, not a bug.
- **Gemini and schemars disagree twice.** After `Option<Vec<T>>` (Phase 2),
  `Option<Struct>` — `anyOf` with a `$ref` into `$defs`. Both fail the whole
  request before the first model call. The rule that survived: tool argument
  schemas contain no `anyOf`, `$ref`, or `$defs`; optional things are
  defaults, not unions.
- **Digests re-check.** Six ledger entries re-executed against the live
  engine: five matched to the byte, one temporal entry skipped by design.
  The property ADR-004 promised is a script that exits non-zero.

### Phase 4 — novelty, and the false positive that almost shipped

- **The quiet-window check earned its place on the first run.** With the
  seeded template correctly at #1, the *quiet* window before the fault also
  scored every steady template at 0.84–1.0 — because the baseline window fell
  before the engine's history began, so every template had zero baseline
  events, floored to one, and steady traffic looked like a 64× burst. A
  caveat flag had already named the condition; the scores shipped anyway.
  Now the baseline is clipped to real history and, below 60 s of it, burst
  novelty is *undetermined*, not inflated. The acceptance test the spec asked
  for is the reason this is a footnote and not a demo-day discovery.
- **Drain over-merged the one distinction that matters.** `request
  completed` and `request failed` share one token of two — exactly the 0.5
  threshold — and became `request <*>`. Two rules fixed it, both defensible
  without S1 in mind: the log level is the leading token of a template's
  identity, and a merge needs at least two agreeing positions.
- **Masking did the work on this corpus.** Drain's merge fires in the unit
  tests, not on S1, whose variables are all maskable. Said plainly so the
  algorithm is not credited for the corpus.

### Phase 5 — changepoints, and a detector that was right about the wrong thing

- **The steady-state test failed on the first run — correctly.** Twelve
  minutes of quiet traffic flagged a 1.9× latency doubling on gateway and
  orders, two buckets long, "no deploy within ±120 s". The gateway log
  agreed: 60 → 125 → 103 → 61 ms, while `cargo build` was loading the same
  laptop. Later the same detector reported traffic collapsing to zero and
  resuming two and a half hours later — the machine had suspended. Every
  one a real change in the series, every one annotated with no nearby
  deploy, which is the failure-mode row in the spec ("no nearby change
  event → deprioritised") doing its job. "No changepoints on steady state"
  is a claim about the host as much as the detector; it is measured on an
  idle one and says so.
- **A bucket boundary must never say "before the deploy".** A down step has
  no first anomalous event, so its timestamp is a bucket start — up to 10 s
  before a deploy that landed inside the same bucket. The SOP's
  contradiction check asks exactly "does the changepoint precede the
  deploy?". The tool now reports the precision of every `at` and refuses to
  order a bucket edge against a deploy inside it.
- **The spec's 2-minute guard would have blanked the demo.** The fast
  timeline has 90 s of history before the fault; a 120 s guard leaves no
  baseline. The guard only needs to outlast confirmation — 30 s does — and
  keeping a change flagged for minutes is another tool's job.
- **The series come from the logs.** The scraper's counters are wall-clock
  stamped and gone on restart; the request lines carry the same facts with
  event time, and rebuild from the files. A changepoint is deterministic on
  frozen data, and its ledger entry re-checks. The diagram's arrow moved.
- **Sixteen items said eight things.** Rate and count on the same labels,
  service and route for a one-route service — grouped, the response went
  from headlines truncated at the byte cap to 10 kB with room. The cascade
  came out in causal order from `at` ascending alone: payments, orders 5 ms
  later, gateway 3 ms after that.

### Phases 6–7 — ranking, and the bundle that replaced three calls

- **The weights as specified put the decoy above the deploy.** By score
  alone, S1's novel-but-harmless INFO template (0.856) outranks the fault
  deploy (0.805): with severity at 0.10, novelty wins. What puts the three
  key facts in the top three is a diversity rule — the best template, the
  best changepoint, the best deploy first — and every item says where it
  would sit by score alone. Recorded, not tuned: one scenario is not a
  tuning set.
- **Zeroing the novelty weight moved every score and no position.** Every
  candidate in S1's incident window is first-seen — the templates, the
  error series from zero, the deploys. Novelty is a constant there. The
  acceptance said "visibly reorders"; what S1 can prove is that every score
  drops by exactly `w_n` and the ledger carries the weights. Said so.
- **Same fact, once.** Five novel ERROR templates share the request ids of
  the same failing checkouts; four error-rate steps land within
  milliseconds on connected services. Two facts, each with its cascade
  listed inside. 8,747 events → 6 items, 2.7 MB → 5.6 kB.
- **Relationships by ref, not eid.** Evidence ids are per investigation and
  stripped from digests; linking items by eid would have put the session's
  numbering back into the digest and broken the re-check.
- **A query issued mid-rebuild sees a partial world.** The first bundle
  check ran four seconds after an engine restart and "lost" the payments
  template — the file had not been read yet. The engine now says
  `caught_up`, and the checks wait for it.

### Phase 8 — the experiment, and the experiment's own footprints

- **Correlation was already good; the point was to stop calling it cause.**
  By Phase 7 the bundle put the fault deploy 0.6 s before the first error
  and the agent wrote "correlational" on every RCA, because the SOP made
  it. Phase 8 gave it the tool to earn the other word: take the request a
  client actually sent, replay it twenty times against each always-on
  version, compare. On S1: `v1 0/20, v2 20/20`. Under a second. A request
  that had succeeded, replayed the same way: `0/20 vs 0/20, not_separated`
  — the tool says no as readily as yes, which is the property that makes
  the yes worth anything.
- **The executor is the engine, not the sandbox.** Phase 0 found the
  harness sandbox cannot reach the Compose network, so the "agent writes a
  replay script" story became "agent designs the experiment, engine runs
  it". The experiment is unchanged: same input, versions varied, outcome
  measured. What changed is who sends the bytes. ADR-010 was amended, not
  reversed, and the amendment says what was lost.
- **An experiment leaves footprints in the thing it measures.** Forty
  replays against payments produce forty log lines; the engine tails those
  logs. Left alone, the causal check would have inflated the error count it
  was checking, put a traffic changepoint on `payments-v1` (which has no
  live traffic during the fault), and dirtied the verification window.
  Every replay now carries a `replay-*` request id and the tailer drops
  those lines: 80 sent, exactly 80 excluded, zero `payments-v1` requests in
  the evidence. Measured, because "we tag it" is a claim.
- **Sanitize twice.** The gateway captures four headers and never an auth
  header. The engine strips auth-shaped headers and redacts secret-shaped
  body fields again on the way out anyway, with unit tests, because the
  capture allowlist is a property of *this* gateway and the tool has to
  survive a real one.
- **The default window was wrong for exemplars.** "Last fifteen minutes"
  is right for everything else; for the exemplar of a failure you want the
  *first* request that failed that way, whenever it was. The tool's default
  is now all history, the earliest match — which also does not move as data
  arrives, so it re-checks from the ledger.
- **Fairness cuts both ways.** The baseline got `http_request` — one call,
  one request, like `curl` — so it *can* test a version pair if it thinks
  to. What it does not get is the twenty-per-version comparison with a
  threshold and a verdict. That is the treatment.

### Phase 9 — the gate, hardened by taking the pen away from the model

- **The model's "fresh UUID" was `9b8c2d1e-3f4a-5b6c-7d8e-9f0a1b2c3d4e`.**
  Ascending nibbles. Phase 2 saw it and wrote it down; Phase 9 removed the
  model from the loop that mints keys. The agent *proposes* — the deployer
  mints a v4 `proposal_id`, snapshots the live version, stamps an expiry,
  journals it — and the gated `rollback` consumes the proposal by id. A
  key the model cannot invent cannot collide.
- **The human approves what they can read.** The rollback call restates
  service, version, expected-current and the evidence ids, and the deployer
  refuses if the restatement differs from what was minted. The runner goes
  one step further and prints each cited evidence id's ledger line at the
  gate: *E8: replay v2 20/20 failed*. Approving evidence, not vibes, is a
  sentence in the README; this is what it looks like on a terminal.
- **The harness gate never times out.** A pending approval sits in a map
  until answered. So "an expired approval is never executed" had to be the
  deployer's property, not the harness's: proposals expire in ten minutes,
  and the check runs where the action runs. Tested with a one-second TTL.
- **Every refusal is a journal line with a reason.** Double-fire → one
  rollback, one `noop`. An operator fixes it by hand while the gate is
  pending → `aborted: version mismatch`, no deploy id minted. Restated
  evidence that differs → `aborted: restated proposal differs`. Expired →
  `aborted: proposal expired`. All live, all in `just s9-check`.
- **Recovery is not the agent's to declare.** `verify_recovery` moved the
  verdict into the engine: three windows resolved from the journal, a
  tolerance on the pre-incident baseline, two consecutive clean checks to
  close, and an `escalation` entry — terminal — when the rate is no better
  than the incident, rising, or still dirty after five minutes. The agent
  sleeps fifteen seconds and asks again; the benchmark reads the ledger's
  closing entry, not the prose. Re-introducing the fault right after a
  "fix" produced exactly the trace the spec asked for: `not_recovered`, then
  `worsening`, then stop.
- **A budget the prompt cannot negotiate.** The engine refuses the 61st
  call in a minute and the 201st in an investigation, with an instruction
  to synthesise from what the agent already has. The harness's
  `iteration_limit` sits above it; this is the floor.

### Phase 10 — the benchmark ✅

Three more scenarios, a runner, a scorer, and the ablation. What the day
taught:

- **The interesting incidents are the ones without a smoking gun.** S1 has
  a stack trace at the culprit; any competent agent finds it. So S2's
  culprit emits *nothing new* — a config-only release (`orders v1.2`: the
  fraud vendor's v2 API and a doubled timeout) that turns into a latency
  cascade and edge timeouts, with the only novel ERROR template at the
  wrong service. S3 has *no change event at all* — a 66 MB blob from
  another tenant fills a `noeviction` redis and payments fails closed,
  bursting a template it already logged rarely in steady state. S6 has a
  cause that is *not in the telemetry* — an unobserved vendor degrades,
  orders fails open silently, and the only evidence is a latency
  changepoint plus a benign deploy six minutes earlier begging to be
  rolled back.
- **Every one of them reproduces to the request.** Two runs from clean
  state, 0.0 points of drift on S2 (seeded traffic, deterministic noise,
  fixed timelines).
- **"Implemented as a `disable_tools` entry" was wrong.** The bundle
  embeds the novelty miner's output, so hiding the tool leaves the
  treatment in place. Ablation A1 became a second instance of the same
  engine binary started with `--ablation no-novelty`: no `novel_templates`,
  no template candidates in the bundle, `w_n = 0`, stamped on every
  watermark.
- **Score mechanically or not at all.** Both SOPs end with a fenced
  `verdict` block (`culprit_service`, `culprit_change`, `action`,
  `evidence_label`); the ground truth lists the accepted values; every
  cited evidence id is resolved to the item the engine returned and
  matched against pre-registered `match` maps. No LLM judge.
- **The gate is simulated as approving** in the matrix, so a wrong
  proposal executes and is counted as a wrong action — the conservative
  reading.
- **The model catalog exposes no prices.** The cost column reads `n/a`
  until `bench/price-sheet.json` is filled from the provider's sheet;
  tokens stand as the proxy rather than an invented dollar figure.

## The benchmark

Same model (`gemini-3-6-flash`), same harness, same incident, same
information access, same gated action path; three conditions — raw tools,
the evidence plane, the evidence plane with novelty switched off — over four
scenarios, three repeats each, one fresh incident per cell, 36 runs in 4 h
23 min unattended, every run committed. The scorer is mechanical: a fenced
verdict block at the end of every report, and every cited evidence id
resolved to the engine's record and matched against ground truth written
before the first run. n = 3 is a hackathon budget, not a study; nothing
below claims significance. The full tables, with ranges and the file behind
every number, are in `docs/benchmark.md`.

| | S1 deploy regression | S2 config-only release | S3 redis full, no change | S6 unobserved vendor |
|---|---|---|---|---|
| Success — baseline / Spyglass / no-novelty | 3/3 · 3/3 · 3/3 | 3/3 · 3/3 · 3/3 | 3/3 · 3/3 · 3/3 | **0/3 · 1/3 · 3/3** |
| Tool calls | 19 · 18 · 21 | 26 · 30 · 39 | 14 · **8.7** · 10 | 29 · 27 · 22 |
| Input tokens | 424k · 461k · 525k | 891k · 937k · 1253k | 210k · **139k** · 157k | 1328k · 705k · 400k |
| Alert → RCA | 63 s · 78 s · 83 s | 102 s · 109 s · 126 s | 49 s · 46 s · 49 s | 125 s · 99 s · 81 s |

## Results, including anything negative

**The prediction was "faster, cheaper, more accurate". Two of those three
did not happen on the scenarios with a cause in the telemetry.** The same
model with `tail`, `grep` and `curl` found the deploy regression, the
config-only release and the redis fill nine times out of nine. Accuracy on
S1–S3 is a tie. Spyglass was cheaper on S3 — 8.7 calls and 139k tokens
against 14 and 210k, every claim cited — and *more* expensive on S1 and S2,
entirely in the action path: the causal check and the engine-judged
verification, which the baseline does not have (it re-reads a metric twice
and declares victory). What the evidence plane changes on those three is
the shape of the work and what the postmortem is made of: 7–22 evidence
ids per report, 91–100 % of the root-cause citations relevant, each
re-checkable against the ledger. The baseline's claims cannot be re-checked
at all. That is a property worth having; it is not the speed-up the README
predicted.

**The negative result is S6, and it is the most useful number in the
table.** A latency-only symptom, no error, no change event, an unobserved
vendor as the cause, and a benign deploy 130 seconds earlier as bait. The
right answer is to refuse and say what would decide it. The no-novelty
ablation refused three times out of three, each time with the
correlation-window argument in its own words. Spyglass refused once and
rolled the benign deploy back twice — once on five log-search hits it
narrated as a cascade "following D-1", once on a `postgres insert slower
than budget` warning that had been in the logs before and after the fault.
The baseline never refused. **More evidence made the agent act.** The
evidence plane handed the model material for a story, and the model told
it. ADR-001 named exactly this failure mode as the thing the thesis had to
avoid; the benchmark's job was to be able to catch it, and it did.

**Two engine gaps came out of the same cell.** After the wrong rollback the
engine "verified" recovery — its check judges the 5xx share and nothing
else, and a latency-shaped incident has none. And nothing mechanical stops
a proposal whose cited evidence contains no deploy-correlated change; the
human at the gate is the only floor, and the matrix, by design, had none.
Both are recorded rather than patched: the matrix stays the matrix.

**Things the measurement itself got wrong.** The pre-registered evidence
precision counts the causal check's and the verification's ids as
non-relevant, so it prints 46 % on S2 where the root-cause citations are 88–100
% — the metric's denominator, reported as scored, with the post-hoc number
beside it and labelled. Time-to-first-hypothesis equals wall time because
this model emits no interim prose. And the ablation runs' ledger re-checks
all read FAIL because the checker talked to the main engine instead of the
ablation engine that had issued the entries; re-run against the right one,
11 match, 0 mismatch. The recorded verdicts stay recorded.

**And one thing that never worked.** The harness sandbox failed its own
bootstrap — `pip install pydantic` through its proxy, connection refused —
in every run since Phase 3, so no sandboxed command ever ran: the SOP's
`sleep 15` between verification checks fell back to the model polling the
engine (which is where a third of Spyglass's S1/S2 calls went), and the
baseline's one attempt to read a log file through the filesystem was
refused with the same error. Symmetric, so the comparison stands; a hole
in the harness integration all the same, diagnosed in Phase 11.

## Limitations

n=3 · one model · self-authored scenarios (mitigated by pre-registered
ground truth and committed raw runs, not eliminated) · one incident domain ·
the causal replay runs on the evidence plane rather than in the harness
sandbox (Phase 0) · the sandbox never executed a command in a recorded run
(Phase 10) · the gate was simulated as approving in the matrix, so "wrong
action" means "wrong proposal" · the scenarios' fast timelines are minutes
long; a real incident's baseline is days.

## What would be built next

Agent-session forensics: point the same evidence plane at TrueForge session
logs to diagnose *other agents'* loops, token burns, and failure patterns.
Same engine, different telemetry.
