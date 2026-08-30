# Demo plan

**Status:** scaffold — shot list from the spec, per-segment commands filled in
as each phase lands. Recorded across Phases 2, 9 and 11.

≤3:00, **failure-first**. Each segment exists for a stated reason.

| Time | Segment | On screen | Why it exists |
|---|---|---|---|
| 0:00–0:10 | Incident begins | green dashboard → `deploy payments v2` → error curve climbs | Stakes in 10 seconds |
| 0:10–0:30 | Naive agent drowns | Phase-2 footage at 8× (labelled): raw log walls, repeated calls, token counter spinning | The foil — makes the thesis visible before it is argued |
| 0:30–0:45 | The turn | one card: telemetry → evidence engine → shaped evidence → agent | The idea, once, in one breath |
| 0:45–1:30 | Spyglass investigates | analysts fan out; `novel_templates` with `first_seen` and `engine_latency_ms`; changepoint +118s after D-77 | Evidence tools carrying the load |
| 1:30–2:00 | Sandbox experiment | replay proportions v1 vs v2 | Correlation → causation; the intellectual peak |
| 2:00–2:25 | Approval + rollback + verify | gate full-screen with cited eids (each resolved to its ledger line) → one human click → `verify_recovery` closes the incident | Control-and-safety, on camera |
| 2:25–2:45 | The ledger | postmortem citing E1–E7; one `get_evidence(E3)` dereference | Auditability made concrete |
| 2:45–3:00 | The numbers | baseline vs Spyglass table (measured values only) + repo end card | The claim settled by measurement |

## Per-segment commands (filled in as each phase lands)

### 0:00–0:10 — Incident begins ✅ available (Phase 1)

Terminal A, the dashboard; terminal B, the injection. Filmed against the
default timeline the fault is eight minutes in; for the shot, inject directly:

```bash
just up                                   # once; stack healthy, all v1
just watch                                # A: green — 0.0% 5xx, payments=v1
./target/release/deployer --data-dir data/deploy deploy payments v2 --actor deploy-bot   # B
#  A: within one 5 s window the bar climbs to ~20%; two windows later the
#     alert fires and (with SPYGLASS_AGENT set) the investigation session opens
```

Reset between takes: `./target/release/deployer --data-dir data/deploy rollback payments v1 --request-id $(uuidgen)`.

### 0:10–0:30 — Naive agent drowns ✅ available (Phase 2) — **film this once, keep every take**

Three windows: the TrueForge session page (the star), the runner terminal, and
`just watch`. The footage is played at 8× in the cut, so let it run long.

```bash
just mcp-up && just tf-setup                    # once: rollback + raw tools registered, agent 'spyglass-baseline'
S1_FAST=1 just scenario s1                      # fresh incident (≈4 min); or deploy payments v2 by hand
just watch                                      # window C: alert fires within ~15 s of the fault
just investigate baseline --approval ask        # window B: prints the session URL -- open it in window A
#   the agent pages raw logs; the terminal prints per-turn tool calls and tokens
#   if it proposes a rollback, the gate shows in A and B asks y/N -- approving is fine for the take
#   result: bench/results/s1-baseline-<run>.json  (this is demo segment 8's baseline row)
```

What the camera should catch: raw log walls scrolling in the session, the
tool-call counter climbing, the token totals in the terminal. Freeze the last
frame on the totals line.

**Narration, honestly** (Phase 2 F6): the baseline *does* find it on S1 — in
~40 s, for ~200k input tokens and 57 KB of raw log paged into context. The
line is "it got there — at two hundred thousand tokens; watch the context
grow", not "it failed". The accuracy story belongs to S2/S3/S6.

Between takes: `S1_FAST=1 just scenario s1` again (clean state, new run id).
### 0:45–1:30, 2:00–2:45 — Spyglass investigates · approval · ledger ✅ skeleton available (Phase 3)

```bash
just demo                     # fresh S1 -> Spyglass agent -> gate (you approve) -> verify -> ledger re-check
# camera: the session page (evidence responses with meta.engine_latency_ms visible),
#         the gate showing justification_eids, the terminal's ledger line,
#         then: just ledger-check ledger/<id>.jsonl   (digests match, on screen)
```

**The novelty screen (0:45–1:00) ✅ available (Phase 4).** The single most
legible frame in the video: the `novel_templates` response in the session
page — rank 1 the seeded ERROR template with `has_stack: true` and
`first_seen_in_window` 0.1 s after the deploy, `meta.engine_latency_ms` in
single digits. Zoom on `novelty_reason: first_seen_in_window` and the
`instances: ["payments-v2"]` line.

**The changepoint line (1:00–1:15) ✅ available (Phase 5).** The
`detect_changepoints` response: item 1's `headline` reads
*`error_rate{route="/charge",service="payments"} up 0.0% → 17.1% (from zero)
at …, +0.6 s after D-2 (payments v1→v2)`* — the spec's "+118 s after deploy"
fact, computed by the engine, with `nearest_deploy.relation:
changepoint_after_deploy`. Zoom on the cascade order (payments → orders →
gateway, milliseconds apart) and `engine_latency_ms`.

**The bundle frame (0:45–1:00) ✅ available (Phase 7).** The
`build_evidence_bundle` response: `coverage.events_scanned` → `items_returned`
(*8,747 events → 6 items* on the fast timeline; larger on the default one —
read the number off the screen, never type it), the head — template, changepoint,
deploy — with `score` and `factors` on each, and `relationships` showing
`D-2 -[precedes_within_120s +0.6 s]->` the template and the changepoint.

**The causal-check frames (1:30–2:00) ✅ available (Phase 8).** See below.

### 1:30–2:00 — The experiment — Phase 8 ✅

Two tool calls in the TrueForge session, both visible as rendered tool
results:

1. `get_exemplar_request(eid = <the top ERROR template>)` — hold on the
   sanitized request (`POST /checkout`, body with `"currency":"EUR"`), the
   `chain` (payments-v2 500 with a stack → orders 502 → gateway 502) and
   `outcome.origin_5xx: payments-v2`. Voiceover: *the request a real client
   sent, eighteen milliseconds before v2 raised.*
2. `replay_exemplar(exemplar = <that eid>, service = payments, versions = [v1, v2], n = 20)`
   — hold on `comparison`: `{"v1": "0/20", "v2": "20/20"}`, `Δ 1.0`,
   `verdict: separated`, and the `reading` line ("…causal evidence for THIS
   failure mode, not proof it is the only one; raw proportions at N=20, no
   p-value claimed"). Then the two evidence ids in `meta.eids` and the
   `side_effects` line: the experiment's own traffic was excluded from the
   evidence.

Then the agent's postmortem line that cites them: *Root cause (CAUSAL) … [E4,
E5, E6]*. Read the numbers off the screen, never type them; a second run
may say 19/20 — say what it says.

Why the executor is the engine and not the sandbox (one sentence, honest):
the harness sandbox is network-isolated by design and cannot reach the
Compose stack (Phase 0), so the experiment runs on the evidence plane; the
agent still designs it. If a judge asks, ADR-010 has the whole story.

Optional B-roll: `just s8-check` — the negative control (a request that
succeeded, replayed the same way: `0/20 vs 0/20 → not_separated`). The tool
can say no.
### 2:00–2:25 — The gate and the close — Phase 9 ✅

Three frames, all from one `just demo` run (or `--approval ask` for the
filmed take, so the click is real):

1. **The proposal.** `propose_rollback` returns `proposal_id`,
   `expected_current: v2`, `expires_at`, the eids. Voiceover: *the agent
   asks; the system mints the key.*
2. **The gate, full screen.** The runner's rendering of the pending
   `rollback`: `proposal_id`, `service payments`, `to_version v1`,
   `expected_current v2`, and each justification eid resolved to the ledger
   line that produced it — *E8: replay v2 20/20 failed*. One human `y`.
   (In the TrueForge UI the same call sits in the approval card.) If a
   second take is wanted: answer `n` — the deny reason reaches the agent
   and the run ends report-only, no retry.
3. **The close.** `verify_recovery` × 3: `insufficient_data` → `clean (1/2)`
   → `recovered … CLOSED`, then the ledger tail showing `verified_recovery`.
   Voiceover: *the agent never declares recovery; the engine does.*

B-roll for the safety story (`just s9-check`, ~2 min, terminal only):
double-fire → `executed D-n` then `noop: duplicate proposal_id`; operator
fixes it by hand while the gate is pending → `aborted: version mismatch`;
a one-second-TTL proposal → `aborted: proposal expired`; the fault
re-introduced after a fix → `not_recovered`, `worsening`, `ESCALATED` — and
the 61st call in a minute refused. Every line is a journal or ledger entry.

### 2:45–3:00 — The numbers — Phase 10 ✅ (`bench/report.py` output only; no hand-typed numbers)

One card. The README's generated results table (`README.md` →
*Results*, between the `bench-results` markers), cropped to the S1–S3 rows
of **baseline** and **spyglass**: Success, RCA acc., tool calls, total
tokens, latency. Read the numbers off the table; the table is regenerated
by `just report` from `bench/results/` and every value traces to a run
file. If a cell favours the baseline, it stays in the shot.

Second beat on the same card, if there is time: the **S6** rows — the
refusal scenario — with the Success column, and the **ablation** column on
S1 vs S2 (what novelty buys where the smoking gun exists, and what the
changepoints carry where it does not). Voiceover: *same model, same harness,
same information, same gate; only the evidence interface changed; n = 3,
every run committed, including the ones that went wrong.*

End card: repo URL, `just demo`, and the one-line thesis.

B-roll: `just bench --dry-run` (the 36-cell plan) and a `bench/results/`
listing — the honesty is visible as a directory.

## Production rules

- Baseline footage is captured in **Phase 2**, not reshot on Sunday.
- Voiceover recorded separately from screen capture, two takes.
- Every clip kept.
- **If a segment's feature was dropped, the segment is cut — never faked.**

## Filming-day runbook (Phase 11)

> **Filming today?** [`demo-day.md`](demo-day.md) is the full operator's
> runbook — cold-machine setup, recorder setup, the four captures with what to
> hold on, the second-by-second cut, the word-for-word narration, ffmpeg
> recipes, and the failure table. This section is its short form.

Everything below was exercised in Phase 11 (`docs/phase11-findings.md`).
Total screen time ≤ 3:00; the cut is assembled from **four captures**, each
of which can be retaken independently. Voiceover is recorded separately
against the assembled cut, two takes.

### Preflight (once, ~5 min; repeat after any reboot)

```bash
source scripts/env.sh                       # node 22 on PATH, harness URL
scripts/trueforge.sh start                  # harness on :8790 — "healthy"
scripts/trueforge.sh status | grep -A1 sandbox   # "enabled": true — else scripts/install-sandbox-deps.sh
just build                                  # engine + deployer + rawtools + target image
just mcp-up && just tf-setup                # 4 MCP servers, 3 named agents (idempotent)
just up                                     # stack green: all v1
```

Three terminals, one browser tab: **A** the TrueForge session page
(`http://localhost:8790/sessions/<id>` — `just investigate` prints the URL),
**B** the runner (`just demo` / `just investigate …`), **C** `just watch`.
Font ≥ 16 pt; the session page is the star, keep it at ≥ 60 % of the frame.

### Capture 1 — the incident begins (segment 0:00–0:10)

```bash
just up && just watch                                       # C: 0.0 % 5xx, payments=v1
./target/release/deployer --data-dir data/deploy deploy payments v2 --actor deploy-bot   # B
# C climbs within one 5 s window; the alert line fires two windows later
./target/release/deployer --data-dir data/deploy rollback payments v1 --request-id $(uuidgen)   # reset
```

### Capture 2 — the naive agent (segment 0:10–0:30; played at 8×)

Already filmed in Phase 2? Use that footage. Otherwise:

```bash
S1_FAST=1 just scenario s1                  # fresh incident (~4 min incl. steady state)
just investigate baseline --approval ask    # B prints the session URL → open in A
# A: raw log walls; B: per-turn tool calls and token totals; freeze on the last totals line
```

Say what the run shows: the baseline *finds* S1 — the cost is the story
(tool calls, tokens, bytes paged into context), not failure.

### Capture 3 — THE loop (segments 0:45–2:45, one continuous take)

```bash
just demo                                   # fresh S1 → Spyglass agent → gate → verify → ledger re-check
```

What to hold on, in order, all in **A** unless noted:

1. `freshness_watermark` → `build_evidence_bundle`: *events_scanned → items_returned*, the head (template / changepoint / `D-2`), `relationships`.
2. `get_evidence` on the ERROR template: the stack, `first_seen`, `instances: ["payments-v2"]`.
3. `get_exemplar_request`: the sanitized request, its `chain`, `outcome.origin_5xx`.
4. `replay_exemplar`: `comparison {"v1": "0/20", "v2": "20/20"}`, `verdict: separated`, the `reading`.
5. `propose_rollback` → the gate in **B**: proposal id, service, versions, each eid resolved to its ledger line. Type `y`. (Second take, optional: `n` — the run ends report-only.)
6. `verify_recovery` × 2–3 in **A**: `insufficient_data`/`clean (1/2)` → `recovered … CLOSED`; **C** falls to 0 %.
7. The postmortem's *Evidence* list, then **B**'s closing lines: `ledger re-check: N match, 0 mismatch → PASS`.

Reset between takes: `S1_FAST=1 just scenario s1` (clean state, new run id).
Read every number off the screen; a second run may say 19/20 — say that.

### Capture 4 — the numbers (segment 2:45–3:00)

Open `README.md` → *Results* (the generated table between the
`bench-results` markers) at a zoom where the **Success**, **No wrong
action**, **RCA** and **Tokens** columns of the S1–S3 and S6 rows are legible;
optionally `ls bench/results | wc -l` in **B** and `just bench --dry-run`.
End card: repo URL · `just demo` · the one-line thesis.

### Narration (≈ 420 words at a normal pace = 2:50)

<!-- narration:begin -->
**0:00 — Incident.** *Friday, 4 p.m. A payments deploy goes out. Within ten
seconds one checkout in five is failing. Someone gets paged.*

**0:10 — The naive agent.** *Give the same model raw tools — tail, grep,
metrics — and it does find it. Watch the cost: nineteen calls, four
hundred thousand tokens, a minute of wall time, and a report you cannot
check. Published evals say this is the ceiling: frontier models under
fifty percent on real incidents, and longer trajectories don't help.*

**0:30 — The turn.** *So don't make the agent smarter. Make the evidence
better. Spyglass is a Rust evidence plane between the telemetry and the
model: it mines templates, scores novelty, finds changepoints, ranks, and
hands the agent a bounded bundle — every item with an evidence id, a
digest, and the engine's latency.*

**0:45 — Spyglass investigates.** *One call: eight thousand events become
six items. The new error template, first seen on payments-v2 a tenth of a
second after the deploy. The error-rate changepoint, plus zero-point-six
seconds after D-2. The deploy itself. The engine says which precedes
which. Single-digit milliseconds — that's the Rust argument, on screen.*

**1:30 — The experiment.** *Correlation is not cause. The agent takes the
request a real client sent, sanitized, and replays it twenty times against
each version. v1: zero of twenty. v2: twenty of twenty. Separated. Now the
word is "caused" — for this failure mode, and the tool says only that.*

**2:00 — The gate.** *The agent cannot act. It proposes; the system mints
the key, snapshots the live version, stamps an expiry. The human reads the
evidence behind each id — E8: replay, v2 twenty of twenty failed — and
says yes, once. Rollback. Then the engine, not the model, verifies: two
clean checks, incident closed.*

**2:25 — The ledger.** *Every claim in the postmortem cites an id; every id
is a ledger line with a digest; re-run the query and the digest matches.
An investigation you can audit next week.*

**2:45 — The numbers.** *Same model, same harness, same information, same
gate; only the evidence changed. Thirty-six runs, every one committed.
Where the cause is in the telemetry, raw tools find it too — a tie on
accuracy; Spyglass cheaper on one scenario, dearer on two. Where there is
no cause to find, the honest result: the no-novelty ablation refused three
of three; Spyglass once; raw tools never. More evidence made the agent
act. That's the finding, and the benchmark is why we know it.*

**End card.** *github.com/vivekjami/spyglass — `just demo`. Evidence
engineering: make the evidence better instead of the model smarter.*
<!-- narration:end -->
