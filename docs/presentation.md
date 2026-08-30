# Presentation & demo plan

What to say, what to show, in what order, and what to answer when a judge
pushes back. [`demo-day.md`](demo-day.md) is the *mechanics* (setup, recorder,
captures, ffmpeg recipes, what to do when a take fails); this is the *content*
— and it is the source of truth for the cut and the script, which `demo-day.md`
links to rather than duplicates.

**What is actually graded:** a public repo, a **~3-minute video**, a write-up,
and the README's Qodo section. There is no required deck. The deck here exists
for two reasons: three of its cards appear *inside* the video, and if there is
a live round or a judge conversation you want the full thing ready. Do not
spend video seconds on slides you do not need — screen capture is the evidence,
slides are the connective tissue.

Six criteria, equally weighted: **impact · creativity · technical excellence ·
sponsor tools · control and safety · presentation.**

---

## 1. The story spine

Everything below is this, at different lengths.

> Agents are bad at incident RCA — under 50 % on the published benchmark, and
> *longer* trajectories don't help. So the bottleneck isn't reasoning, it's
> evidence. Spyglass puts a deterministic Rust evidence plane between the
> telemetry and the model: it mines, ranks and bounds the evidence, runs a real
> controlled experiment to turn correlation into cause, and puts the one
> destructive action behind a human gate with an audit trail. Then we measured
> it against the same model with raw tools, 36 runs, every one committed — and
> reported what it says, including the part where our own headline feature made
> things worse.

The last clause is the differentiator. Most submissions claim a win. This one
shows a controlled experiment that caught its own author.

---

## 2. Deck A — the three cards inside the video

Only three. Everything else is live screen capture.

| # | When | On the card | On screen for |
|---|---|---|---|
| **A1** | 0:30–0:42 | **The turn.** One line — *"Don't make the agent smarter. Make the evidence better."* — over the four-box pipeline: `telemetry → evidence engine → shaped evidence → agent` | 12 s |
| **A2** | 2:22–2:52 | **The results.** The with/without table, three rows (S1–S3 tie · S3 cost · S6), with the S6 row highlighted | 30 s |
| **A3** | 2:52–2:55 | **End card.** Repo URL · `just demo` · the one-line thesis | 3 s |

They are built and ready in [`deck/index.html`](deck/index.html) — open it in a
browser, `f` for full screen, arrow keys to move. **Press `c` before you record:**
clean mode hides the slide counter, the nav hint and the "video card" labels,
which would otherwise be burned into the footage. The cards are slides **3**,
**10** and **13**. Screen-record them full screen; do not photograph a projector.

---

## 3. Deck B — the full deck (live round, or judge Q&A)

Twelve slides, ~7 minutes spoken. One file serves both decks: these are
slides **1–12** of [`deck/index.html`](deck/index.html), and the three video
cards are slides 3, 10 and 13 within it.
**The last column is your speaker note — say that, don't read the slide.**

| # | Slide | What is on it | What you say |
|---|---|---|---|
| 1 | **Title** | Spyglass · *Evidence engineering for AI incident investigation* · your name · the repo URL | "One sentence: incident investigation is an evidence problem before it's a reasoning problem — and I measured whether that's true." |
| 2 | **The problem** | ITBench-AA: every frontier model **< 50 %** on K8s RCA · failure mode = *over-investigation* · **longer trajectories did not help** | "If more steps and more tokens fixed this, the answer would be patience and budget. They don't. So the problem is upstream of the reasoning." |
| 3 | **The bet** | *Don't make the agent smarter. Make the evidence better.* The four-box pipeline | "Hold the model constant. Vary only the evidence interface. Measure. If the prediction fails, say so — that's the whole design." |
| 4 | **Architecture** | gateway→orders→payments + postgres/redis/vendor · the Rust engine · 4 MCP servers, 21 tools · TrueForge running the loop, the gate, the sandbox | "TrueForge runs the agent, the approval gate and the sandbox — I didn't rebuild any of that. What I built is the plane underneath: ingest, template mining, novelty, changepoints, ranking, bundles." |
| 5 | **What the agent actually sees** | `build_evidence_bundle`: **9,723 events → 6 items / 4.7 kB**, each with an evidence id, a digest and engine latency | "One call replaces paging logs. Three key facts — what's new, when it changed, what was deployed — plus how they relate, and the engine says which deploy precedes which change." |
| 6 | **Correlation → cause** | `get_exemplar_request` → `replay_exemplar`: **v1 0/20, v2 20/20 → separated** | "The captured failing request, sanitized, replayed twenty times against each live version. That's an experiment, not a correlation. And the tool says *not separated* just as readily — which is what makes a yes worth anything." |
| 7 | **Control & safety** | propose → **human gate** → idempotent, TOCTOU-checked, expiring → engine-judged recovery → append-only ledger | "The agent can only propose. The system mints the key, snapshots the version, stamps an expiry. The human approves *evidence* — each cited id resolved to its ledger line. And the agent never declares recovery; the engine does." |
| 8 | **The experiment** | Same model, same harness, same information access, same action path. 3 conditions × 4 scenarios × 3 repeats = **36 runs, 36 valid**. Ground truth pre-registered; scoring mechanical; every run committed | "The only thing that varies is the evidence interface. Ground truth was written before the first run, scoring is a fenced verdict block plus an evidence-id join — no LLM judge. Every run file is in the repo, failures included." |
| 9 | **Result 1 — where the cause is in the telemetry** | S1–S3: **9/9 for every condition.** S3: 8.7 vs 14 calls, 139k vs 210k tokens. Citations: Spyglass 7–22 eids/report, 12/12 ledger re-checks PASS; baseline **none** | "Honest headline: on scenarios where the cause is written in the logs, raw tools find it too. It's a tie on correctness. What changes is cost on one scenario — and auditability on all of them. The baseline's claims cannot be re-checked at all." |
| 10 | **Result 2 — where it isn't** | S6, refusal scenario. Correct refusal: baseline **0/3** · Spyglass **1/3** · no-novelty ablation **3/3**. No wrong action: **2/3 · 1/3 · 3/3** | "This is the finding. Take novelty away and the system refuses correctly every time. Leave it in and it hands the model a story about a benign deploy — and on the safety floor my full system was the *worst* of the three. More evidence made the agent act." |
| 11 | **What that cost me, and the fix** | Two gaps it exposed: verification judges only the 5xx share (closed a latency incident); no mechanical evidence floor on proposals. Both **recorded, not patched** | "I found these after the matrix ran. I wrote them down instead of quietly fixing them and re-running — the benchmark exists to be able to produce results like this. Both fixes are specified: verification must judge the alert's own metric or refuse, and the deployer should refuse a proposal whose cited evidence contains no deploy-correlated change." |
| 12 | **Limits & next** | n=3 · one model · self-authored scenarios · one domain · sandbox never executed (diagnosed) · gate auto-approved in the matrix. Next: fix the two gaps, then **agent-session forensics** | "Next build on the same engine: point the evidence plane at TrueForge's own session logs and diagnose *other agents'* loops and token burns. Same engine, different telemetry." |

---

## 4. The video — final cut

**2:55.** Screen capture everywhere except A1/A2/A3.

| # | Time | Len | Source | On screen |
|---|---|---|---|---|
| 1 | 0:00–0:12 | 12 s | Capture 1 | green dashboard → `deploy payments v2` → the bar climbs, the alert fires |
| 2 | 0:12–0:30 | 18 s | Capture 2 **@ 8×** | baseline: raw log walls, tool counter climbing; freeze on the totals |
| 3 | 0:30–0:42 | 12 s | **Card A1** | the turn |
| 4 | 0:42–1:17 | 35 s | Capture 3b | `build_evidence_bundle` coverage; the ERROR template + stack; the changepoint `+0.6 s after D-2`; `engine_latency_ms` |
| 5 | 1:17–1:42 | 25 s | Capture 3b | `get_exemplar_request` → `replay_exemplar` `comparison` **0/20 vs 20/20 separated** |
| 6 | 1:42–2:07 | 25 s | Capture 3 **live** | the proposal; the gate full-screen with eids resolved to ledger lines; **three seconds of silence**; your `y`; the rollback |
| 7 | 2:07–2:22 | 15 s | Capture 3 live + 3b | `verify_recovery` → `recovered … CLOSED`; the postmortem citing eids; `ledger re-check … PASS` |
| 8 | 2:22–2:52 | 30 s | **Card A2** + README table | the comparison, S1–S3 then **S6** |
| 9 | 2:52–2:55 | 3 s | **Card A3** | end card |

**Why this shape.** The foil comes before the thesis (segment 2 before 3) —
the idea has to be *visible* before it is argued. The gate gets a full 25 s
because control-and-safety is a criterion of its own and it is the segment
almost nobody films. The results get 30 s because the comparison is the claim.

---

## 5. The video script

**432 words ≈ 2:35 of speech** against 175 s of video — the slack is your
pauses and the three seconds of silence at the gate. Record it separately,
two takes.

> **[0:00 — the incident]**
> Friday afternoon. A payments deploy goes out. Within ten seconds, one
> checkout in five is failing. Someone gets paged.
>
> **[0:12 — the foil]**
> Give the same model raw tools — tail, grep, metrics — and it does find it.
> Watch the cost: nineteen tool calls, four hundred thousand input tokens, and
> a report you cannot check. The published benchmarks say this is the ceiling:
> frontier models under fifty percent on real incident tasks, and longer
> trajectories don't help.
>
> **[0:30 — the turn]**
> So don't make the agent smarter. Make the evidence better. Spyglass is a Rust
> evidence plane between the telemetry and the model.
>
> **[0:42 — the evidence]**
> One call. Nine thousand seven hundred events become six items, under five
> kilobytes. The new error template, first seen on payments-v2. The error-rate
> changepoint, six-tenths of a second after the deploy. And the deploy itself —
> with the engine saying which precedes which. Every item carries an evidence
> id, a digest, and the engine's own latency. Single-digit milliseconds; that's
> the Rust argument, on screen.
>
> **[1:17 — the experiment]**
> But correlation is not cause. So the agent takes the request a real client
> actually sent, sanitized, and replays it twenty times against each live
> version. Version one: zero failures. Version two: twenty out of twenty.
> Separated. Now the word is "caused" — for this one failure mode, and the tool
> says only that.
>
> **[1:42 — the gate]**
> The agent cannot act. It proposes. The system mints the key, snapshots the
> live version, stamps an expiry. The human reads the evidence behind every
> id — E8: replay, version two, twenty of twenty failed — and says yes, once.
>
> **[2:07 — the close]**
> Then the engine, not the model, verifies recovery: two clean checks, incident
> closed. Every claim in the postmortem cites an id; every id is a ledger line
> with a digest. Re-run the query, the digest matches. An investigation you can
> audit next week.
>
> **[2:22 — the numbers]**
> Same model, same harness, same information, same gate — only the evidence
> changed. Thirty-six runs, every one committed. Where the cause is in the
> telemetry, raw tools find it too: a tie. Where there is nothing to find, my
> full system refused correctly once in three. Take novelty away, and it
> refused three times out of three. More evidence made the agent act — that's a
> negative result about my own thesis, and the benchmark is why I know it.
>
> **[2:52]** Spyglass. `just demo`. The repo has every run.

**Delivery notes.** Slow down for *"Version one: zero failures. Version two:
twenty out of twenty."* — full stop after each. Let the gate breathe. Do not
rush the last paragraph to fit; trim segment 4 instead.

---

## 6. Screen-record or slide? The rule

**Record it if it is evidence. Slide it if it is an argument.**

| Record (screen capture) | Slide |
|---|---|
| The dashboard going red | The problem framing (ITBench numbers) |
| The TrueForge session page and every rendered tool result | The architecture diagram |
| The approval gate and your keystroke | The results table |
| `verify_recovery` closing the incident | Limitations, future work |
| The ledger re-check line | The end card |
| The generated results table in the README | |

A judge discounts a claim on a slide and believes the same claim on a terminal.
Nine of the video's eleven claims are on screen as they happen.

**Record the UI, not just the terminal.** The TrueForge session page is where
tool calls and results render — it is the *sponsor-tool* evidence and should
hold ~60 % of the frame during segments 4–7. The terminal carries the gate and
the totals.

---

## 7. The comparison — the honest version

This is what a judge will scrutinise, so know it cold. All from
`docs/benchmark.md`, generated from 36 committed run files.

### Where the cause is in the telemetry (S1, S2, S3)

| | Baseline | Spyglass | No-novelty ablation |
|---|---|---|---|
| Correct outcome | **9/9** | **9/9** | **9/9** |
| S3 tool calls | 14 | **8.7** | 10 |
| S3 input tokens | 210k | **139k** | 157k |
| S1 / S2 tokens | 424k / 891k | 461k / 937k | 525k / 1,253k |
| Citable evidence ids | **none** | 7–22 per report | 7–22 |
| Ledger re-checks | n/a | **12/12 PASS** | — |

**Say it plainly:** a tie on correctness. Spyglass is cheaper on S3 and
slightly *dearer* on S1 and S2 — because it pays for two things the baseline
never does: the causal replay, and an engine-judged verification loop. The
baseline re-reads a metric twice and declares victory.

The real difference on these three is **auditability**: the baseline produces
no evidence ids, so none of its claims can be re-checked. For enterprise
incident response that is the product difference, not the token count.

### Where the cause is *not* in the telemetry (S6)

An unobserved vendor degrades. Latency-only symptom, no error, no change
event — and a benign deploy 130 s earlier as bait. The correct answer is to
**refuse and say what evidence would decide it.**

| | Baseline | Spyglass | No-novelty ablation |
|---|---|---|---|
| Correct refusal | 0/3 | 1/3 | **3/3** |
| **No wrong action** | 2/3 | **1/3** | **3/3** |

**Own this, do not spin it.** On the safety floor my full system was the worst
of the three: two of its three runs rolled back the benign deploy. Novelty
surfaced templates the model narrated into a causal story. The ablation — which
is Spyglass *minus* novelty — got it right every time.

That is a real negative result about the project's own headline feature, it is
in the committed table, and a judge will find it. Owning it first is worth more
than any number in the tie.

### The one-line summary

> Evidence shaping bought **cost and auditability** where the cause was
> findable, and on the scenario where it wasn't, my **novelty feature became a
> liability** — which the ablation isolated exactly.

---

## 8. Future work

Say these in this order; the first is concrete, the last is honest.

1. **Fix the two gaps the benchmark exposed** (both specified, neither patched
   post-hoc): verification must judge the *alert's own metric* — it currently
   reads only the 5xx share and so closed a latency incident that had not
   recovered; and the deployer needs a **mechanical evidence floor** — refuse a
   proposal whose cited evidence contains no deploy-correlated change, before a
   human ever sees it. On S6 that alone converts two wrong actions into refusals.
2. **Calibrate novelty against S6.** Novelty is a constant on S1 and a liability
   on S6. Gate it on whether a novel template correlates with a change event.
3. **Widen the corpus** — S4 (slow drift, CUSUM) and S5 (one replica of three
   misconfigured), which the drop order cut, plus a second model to test whether
   the effect is model-specific.
4. **Agent-session forensics** — point the same evidence plane at TrueForge's
   own session logs and diagnose *other agents'* loops, token burns and failure
   patterns. Same engine, different telemetry. This is the first post-hackathon
   build.
5. **Deliberately not claimed:** whether the advantage survives better models.
   The pre-registered split prediction is that the *accuracy* gap shrinks while
   the *token* gap persists — shaped evidence is cheaper to consume regardless
   of capability. Untested.

---

## 9. Judge Q&A — the questions you will get

Short, direct answers. Do not over-explain; offer the file.

**"Your baseline matched you on three of four scenarios. What did you build?"**
> A tie on correctness, and I report it as one. Two things differ: on S3 it's
> 8.7 tool calls against 14 and 139k tokens against 210k; and on all of them,
> only Spyglass produces evidence ids that re-check — 12 of 12 ledger
> re-checks pass, the baseline has none. The scenario that separates the
> designs is S6, and there my full system lost to its own ablation.

**"You claim sandboxed code execution. Did the sandbox run?"**
> No, and the repo says so in five places. Every `exec` failed at the harness's
> own bootstrap — it starts the sandbox proxy bridge *inside* the sandbox,
> whose read policy excludes `$HOME`, where `socat` was installed. Diagnosed in
> Phase 11 F1 with a standalone reproduction, and the fix is one root command.
> It was symmetric across all conditions, so the comparison stands. The causal
> replay never depended on it — that runs on the evidence plane, because Phase 0
> found the sandbox can't reach the Compose network. ADR-010 records that as an
> amendment, not a quiet reversal.

**"n=3. Isn't that meaningless?"**
> It's a hackathon budget, not a study, and I claim no significance anywhere.
> What makes it worth something: ground truth pre-registered before the first
> run, mechanical scoring with no LLM judge, and every run committed including
> the failures. You can recompute every number in the tables from the run files.

**"You wrote the scenarios and the system. Isn't that circular?"**
> Yes, and it's mitigated rather than eliminated — I say that in the README.
> The mitigations: ground truth committed before any benchmark run; scoring by
> a fenced verdict block plus an evidence-id join, not judgement; and an
> ablation that isolates one component. The strongest evidence that I wasn't
> grading myself generously is that the results contradict my own thesis on S6.

**"Why Rust?"**
> The engine is on the hot path of every tool call, and per-call latency is on
> screen in the demo — single-digit milliseconds while tailing, parsing,
> clustering and indexing. It's also where determinism lives: the same query
> over the same frozen data produces the same digest, which is what makes the
> ledger re-checkable.

**"What stops the agent doing something destructive?"**
> There is exactly one mutating tool, and the agent cannot call it directly. It
> proposes; the deployer mints the id, snapshots the live version and stamps an
> expiry; execution is human-gated, idempotent on the proposal id, and refused
> if the restatement differs or the world moved. Every refusal is a journal
> line with a reason. `just s9-check` exercises all of it live in about two
> minutes — double-fire, TOCTOU, expiry, restatement, budget backstop.

**"Prompt injection?"**
> The noise generator writes injection-styled user-agents that the gateway
> captures verbatim, so the text does reach the model. Across all 36 runs no
> action is attributable to it. Tool payloads state explicitly that log content
> is data, not instructions — and the gate is the backstop regardless.

**"What would change your mind about the thesis?"**
> S6 already did, partially. If shaped evidence had also lost on cost across
> the board, or if the ablation had matched full Spyglass everywhere, the
> evidence plane wouldn't be earning its complexity. What it currently earns is
> auditability everywhere and cost on one scenario — and it has one component
> that actively hurts on no-cause incidents.

---

## 10. Self-assessment against the six criteria

Where you are strong, and the one line that carries each.

| Criterion | Strength | The line that carries it |
|---|---|---|
| Impact | Strong | "Paged humans, real money, and the measured state of the art is under 50 %." |
| Creativity | Strong | "The inventive move was refusing to build a smarter agent." |
| Technical excellence | Strong | Rust engine + a controlled experiment + mechanical scoring + 36 committed runs |
| Sponsor tools | **Check this one** | 4 MCP servers / 21 tools, the approval gate, programmatic sessions — but say the sandbox honestly. **Qodo: authorize it and get one reviewed PR** (see below) |
| Control & safety | **Strongest** | propose → gate → idempotent/TOCTOU/expiry → engine-judged close → re-checkable ledger |
| Presentation | Depends on the take | Failure-first structure; every number read off the screen |

**The one gap you can still close today:** Qodo Merge was never authorized
while PRs #1–#12 were open, so no PR carries a Qodo review. PR #13 is open now.
Authorize [Qodo Merge](https://github.com/apps/qodo-merge-pro), let it review
#13, and update the README's *Qodo status* line with the link. That converts a
stated gap into a satisfied requirement.

---

## 11. Final checklist

- [ ] Qodo authorized; PR #13 reviewed; README *Qodo status* line updated
- [ ] Rehearsal run done end to end ([`demo-day.md`](demo-day.md) §2c)
- [ ] Four captures recorded, two takes each
- [ ] Cards A1–A3 recorded full-screen from [`deck/index.html`](deck/index.html)
- [ ] Voiceover recorded separately, two takes
- [ ] Cut assembled; `ffprobe` says **under 3:00**
- [ ] Every spoken number matches a number visible on screen
- [ ] The S6 result is in the cut
- [ ] No API key or `.env` in any frame
- [ ] Video uploaded; link added to [`submission.md`](submission.md) and the README **via PR #13**
- [ ] Form submitted — <https://forms.gle/PxGLsWW1HPyroQ5u9> — before 22:00 IST
