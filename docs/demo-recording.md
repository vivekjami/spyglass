# The recorded demo — what exists, and what is left to do

A complete silent cut of the demo was recorded unattended on 2026-08-30. This
file says exactly what is in it, what was captured how, the two things that
still need you, and the honesty caveats you must respect when narrating it.

**The draft:** `~/spyglass-recordings/spyglass-demo-draft.mp4`
**1:55.77 · 1920×1080 · 30 fps · H.264 · 3.3 MB · no audio.**

Everything on screen is real output from live runs made minutes apart on the
same machine. Nothing is mocked, re-typed or staged.

---

## 1. What is in the cut

| # | In | Len | Source | What is on screen |
|---|---|---|---|---|
| 1 | 0:00 | 11.6 s | `c1-incident.webm` | The watcher: seven windows at **0.0 % 5xx, payments=v1**, then `2.0 → 14.0 → 18.0 %` on `payments=v2 (D-3)` and `*** ALERT *** … 5xx rate 18.0 % (threshold 5 %) for 2 consecutive 5s windows` |
| 2 | 0:11.6 | 8.2 s | `c2-baseline.webm` @ 11× | The **baseline** investigating with raw tools — 12 tool calls, 275 k tokens on turn 1 alone |
| 3 | 0:19.8 | 5.0 s | still, held | **The baseline's approval gate.** It cites `E1`–`E5`, and every one resolves to **`NOT ISSUED BY THE ENGINE IN THIS INVESTIGATION`** |
| 4 | 0:24.8 | 10 s | `card-A1` | *Don't make the agent smarter. Make the evidence better.* |
| 5 | 0:34.8 | 26 s | session page, panned | Root Cause · **Causal Check** (`payments-v1 0/20`, `payments-v2 20/20`, `Verdict: separated (delta = 1.0)`, and its stated limitation) · **Rejected Hypotheses** (deploy `D-1` rejected: it landed 50 s *before* any error changepoint and the cascade originated downstream) · Action Taken |
| 6 | 1:00.8 | 17 s | still, held | **Spyglass's approval gate.** Same six-field shape as the baseline's — but `E1` resolves to `build_evidence_bundle → 7 items / 5,843 B from 9,738 events`, `E10` to `replay_exemplar … v1 0/20, v2 20/20 → separated (Δ 1.00)` |
| 7 | 1:17.8 | 14 s | session page, panned | **Verification**: check 1 `insufficient_data` → check 2 `clean` → check 3 `recovered`, **Incident CLOSED** · the **Evidence Index**, E1–E13 |
| 8 | 1:31.8 | 17 s | `card-A2` | The results: S6, baseline **0/3** · Spyglass **1/3** · no-novelty **3/3** |
| 9 | 1:48.8 | 7 s | `card-A3` | End card |

**Segments 3 and 6 are the spine of the whole argument** — the same gate, from
the same scenario, minutes apart. One cites evidence that does not exist. The
other cites evidence that resolves to a ledger line. That is the auditability
claim shown rather than asserted, and it was not planned — it fell out of
recording both conditions back to back.

## 2. Source clips (kept, all reusable)

In `~/spyglass-recordings/`:

| File | Len | Contents |
|---|---|---|
| `c1-incident.webm` | 32.5 s | full green → deploy → climb → alert |
| `c2-baseline.webm` | 90.7 s | the entire baseline run, ending on its gate |
| `c3-spyglass.webm` | 93.3 s | the entire Spyglass run, ending on its gate |
| `card-A1/A2/A3.webm` | 11/19/7 s | the deck cards, clean mode, no cursor |
| `spyglass-demo-draft.mp4` | 1:55.77 | the assembled cut |

The runs behind them are committed like any other:
`bench/results/s1-spyglass-20260830T053039Z.json` — 11 tool calls, 247,837
input tokens, 69.6 s, 3 verification checks, **CLOSED**, **13/13 eids cited**,
ledger re-check **PASS**, 5xx 20.4 % → 0.0 %.

## 3. What still needs you

### a. The voiceover (required — the cut is silent)

Record `vo.wav` (OBS, mic only, or any recorder), then:

```bash
cd ~/spyglass-recordings
ffmpeg -i spyglass-demo-draft.mp4 -i vo.wav \
       -map 0:v -map 1:a -c:v copy -c:a aac -shortest spyglass-demo.mp4
ffprobe -v error -show_entries format=duration -of csv=p=0 spyglass-demo.mp4
```

The script is in §5 below, timed to *this* cut — not the 2:55 script in
[`presentation.md`](presentation.md), which is for the longer version.

### b. The gate keystroke (optional, but it is the better shot)

You were out, so the runs used `--approval allow`: the gate renders in full
and the runner auto-approves, so the footage reads `-> APPROVED` rather than
showing a human decide. If you want the honest "one human click":

```bash
S1_FAST=1 just scenario s1                                 # ~3.5 min, off camera
scripts/record.py ~/spyglass-recordings/c3b.webm --secs 120 --no-cursor &
just investigate spyglass --scenario s1 --approval ask     # type y when it pauses
```

Then swap segment 6's still for a frame of your take. Everything else stands.

## 4. Honesty caveats — do not narrate past these

1. **The two runs in the footage are n=1 each, not the benchmark.** Back to
   back on the same fault they came out baseline 17 calls / ~501 k tokens
   versus Spyglass 11 calls / ~248 k. That is a nice illustration and it is
   *not* the result. The committed benchmark (n=3) says S1 is a **tie on
   correctness** and Spyglass is slightly *dearer* on S1. If you quote numbers,
   quote the card at 1:31 — it is generated from the 36 committed runs.
2. **The gate was auto-approved** in this footage (see 3b).
3. **The baseline's `E1`–`E5` are the model's own invention**, not a Spyglass
   failure — that is precisely the point: with raw tools there is no evidence
   id to cite, so a citation cannot be checked. Say that, don't imply the
   baseline was sabotaged.
4. **The session page is real**, captured from the live TrueForge UI at
   `localhost:8790/sessions/01m18jc9632p0zq2b96wsv01tz`; the pans are a
   full-page render scrolled in post, not a hand-scroll.
5. **S6 is a loss for the full system** and the card says so. Keep it.

## 5. The narration, timed to this cut

**≈ 285 words ≈ 1:54 at 150 wpm.** Beat markers match the segment table.

> **[0:00]** Friday afternoon. A payments deploy goes out. Within ten seconds,
> one checkout in five is failing, and the alert fires.
>
> **[0:12]** Give the same model raw tools — tail, grep, metrics — and it does
> find it. Twelve tool calls and a quarter of a million tokens in, it asks to
> roll back.
>
> **[0:20]** And here is the problem. It cites five pieces of evidence. Every
> one of them: *not issued by the engine in this investigation*. The model made
> them up. There is nothing to check.
>
> **[0:25]** So don't make the agent smarter. Make the evidence better.
>
> **[0:35]** Same incident, same model — now reading a Rust evidence plane.
> Nine thousand seven hundred events become seven items. It captures the
> request a real client sent and replays it twenty times against each version:
> version one, zero failures; version two, twenty out of twenty. Separated.
> That is an experiment, not a correlation. And it *rejects* the decoy — the
> other deploy landed fifty seconds before any error changepoint.
>
> **[1:01]** The same gate, from the same fault, minutes apart. This time every
> evidence id resolves: the bundle, the exemplar, the replay. The human
> approves evidence, not a hunch.
>
> **[1:18]** Then the engine — not the model — verifies recovery. Two clean
> checks. Incident closed. Thirteen evidence ids, every one a ledger line with
> a digest you can re-run.
>
> **[1:32]** Thirty-six runs, every one committed. Where the cause is in the
> telemetry, raw tools find it too — a tie. Where there is nothing to find, my
> full system refused correctly once in three. Take novelty away and it refused
> three times out of three. More evidence made the agent act. That is a
> negative result about my own thesis, and my own benchmark is how I know it.
>
> **[1:49]** Spyglass. `just demo`. The repo has every run.

**Delivery:** stop hard after *"There is nothing to check."* Let 0:20–0:25 sit
in silence on the red gate. Slow down for *"version one, zero failures; version
two, twenty out of twenty."*

## 6. How it was recorded

`scripts/record.py` drives GNOME's screencast over D-Bus (a one-shot
`gdbus call` records a single frame — the connection has to be held open).
Terminal captures ran in a full-screen `gnome-terminal` whose wrapper never
exits early, so a failing command can never let the recorder fall through to
the desktop behind it. The deck cards were captured from Chrome kiosk at
`deck/index.html?clean#N`. The session page was rendered full-page headless and
panned with ffmpeg. Assembly: `ffmpeg` speed-up, still-holds, concat — all the
commands are in [`demo-day.md`](demo-day.md) §7.

Two traps worth remembering, both of which bit during this session: `pkill -f`
matches the shell that runs it (this repo learned that in Phase 2 and I hit it
twice), and `just watch` takes no arguments — passing one made the terminal
exit instantly and the recorder filmed the desktop behind it. That clip was
deleted unviewed; the probe-before-record step in `demo-day.md` exists because
of it.
