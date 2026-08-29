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
| 2:00–2:25 | Approval + rollback + verify | gate full-screen with cited eids → one human click → recovery curve | Control-and-safety, on camera |
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
### 0:45–2:45 — Phase 3–9
### 2:45–3:00 — Phase 10 (`bench/report.py` output only; no hand-typed numbers)

## Production rules

- Baseline footage is captured in **Phase 2**, not reshot on Sunday.
- Voiceover recorded separately from screen capture, two takes.
- Every clip kept.
- **If a segment's feature was dropped, the segment is cut — never faked.**
