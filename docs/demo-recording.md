# The recorded demo — what exists, and what is left to do

A complete silent cut of the demo was recorded unattended on 2026-08-30. This
file says exactly what is in it, what was captured how, the two things that
still need you, and the honesty caveats you must respect when narrating it.

**The draft:** `~/spyglass-recordings/spyglass-demo-draft.mp4`
**1:56.57 · 1920×1080 · 30 fps · H.264 · 8.5 MB · no audio.**

Everything on screen is real output from live runs made minutes apart on the
same machine. Nothing is mocked, re-typed or staged.

---

## 1. What is in the cut

**Both investigations were run with `--approval ask`, so the gate genuinely
blocks** — the footage shows `approve? [y/N]` sitting there waiting, which an
auto-approved run cannot show. See §3 for who pressed the key.

| # | In | Len | Source | What is on screen |
|---|---|---|---|---|
| 1 | 0:00 | 11.6 s | `c1-incident.webm` | The watcher at **0.0 % 5xx, payments=v1** for seven windows, then `2.0 → 14.0 → 18.0 %` on `payments=v2 (D-3)`, and `*** ALERT *** … 5xx rate 18.0 % (threshold 5 %) for 2 consecutive 5s windows` |
| 2 | 0:11.6 | 9 s | baseline session page | The **baseline working from raw telemetry** — it pastes raw Prometheus lines (`requests_total{…,status="502"} 173.0`), the raw deploy-journal JSON, and a raw `UnsupportedCurrency` stack trace into its own report, labelling them `[E1]`–`[E4]` |
| 3 | 0:20.6 | 8.1 s | `c2-gate.webm` (live) | **The baseline's gate, blocking.** It cites `E1`–`E4`; every one resolves to **`NOT ISSUED BY THE ENGINE IN THIS INVESTIGATION`**. The ids are the model's own invention — with raw tools there is no id to cite, so nothing can be checked |
| 4 | 0:28.7 | 10 s | `card-A1` | *Don't make the agent smarter. Make the evidence better.* |
| 5 | 0:38.7 | 22 s | Spyglass session page | **Root Cause** (`validate_v2` in `/app/payments/app.py`) → **Causal Check**: exemplar `req:17c03e28…`, replayed 20× per version, `payments-v1` **0/20**, `payments-v2` **20/20** |
| 6 | 1:00.7 | 17.9 s | `c3-gate.webm` (live) | **Spyglass's gate, blocking.** Same six fields, same fault — but `E1` resolves to `build_evidence_bundle → 7 items / 4,815 B from 9,786 events`, `E10` to `replay_exemplar … v1 0/20, v2 20/20 → separated (Δ 1.00)`. Then the pause, the `y`, `-> APPROVED` |
| 7 | 1:18.6 | 14 s | Spyglass session page | **Rejected Hypotheses** — deploy `D-1` dismissed because the error onset coincided (+0.8 s) with `D-2` instead → **Mitigation & Action**, proposal `eef65b46…` (the same id shown at the gate) → **Verification**: `insufficient_data` → `clean` → `recovered`, **incident CLOSED** |
| 8 | 1:32.6 | 17 s | `card-A2` | The results: S6 — baseline **0/3** · Spyglass **1/3** · no-novelty **3/3** |
| 9 | 1:49.6 | 7 s | `card-A3` | End card |

**Segments 3 and 6 are the spine.** The same gate, the same fault, minutes
apart. One cites four ids that do not exist. The other cites six that each
resolve to a ledger line. That is the auditability claim shown rather than
asserted — and it was not planned; it fell out of recording both conditions
back to back.

## 2. Source clips (kept, all reusable)

In `~/spyglass-recordings/`:

| File | Len | Contents |
|---|---|---|
| `c1-incident.webm` | 32.5 s | full green → deploy → climb → alert |
| `c2-gate.webm` | 89.9 s | the baseline run with a **blocking** gate (`--approval ask`) |
| `c3-gate.webm` | 102.9 s | the Spyglass run with a **blocking** gate |
| `c2-baseline.webm` | 90.7 s | first take, auto-approved — superseded, kept |
| `c3-spyglass.webm` | 93.3 s | first take, auto-approved — superseded, kept |
| `card-A1/A2/A3.webm` | 11/19/7 s | the deck cards, clean mode, no cursor |
| `spyglass-demo-draft.mp4` | 1:56.57 | the assembled cut |

The two runs in the cut are committed like any other:

| | tool calls | input tokens | wall | outcome |
|---|---|---|---|---|
| `s1-baseline-20260830T100246Z.json` | 15 | **846,349** | 66.1 s | rolled back, citing four ids that do not resolve |
| `s1-spyglass-20260830T095628Z.json` | **11** | **241,067** | 78.5 s | rolled back, 9 eids cited, 3 checks, **CLOSED**, ledger re-check **PASS** |

Both recovered the system (20.6 % → 0.0 % and 21.4 % → 0.0 %).

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

### b. The `y` was sent by a script, not a finger

`--approval ask` makes `investigate.py` block on `input()`. The run genuinely
halts — that part is real, and it is the safety property worth filming. But
you were out, so a controller held a FIFO open, waited for `approve? [y/N]` to
appear, **left it sitting for four seconds** (that pause is the shot), and then
wrote `y`.

So: the gate blocking is real; the hand is not. If you want it to be yours:

```bash
S1_FAST=1 just scenario s1                                 # ~3.5 min, off camera
scripts/record.py ~/spyglass-recordings/mine.webm --secs 130 --no-cursor &
just investigate spyglass --scenario s1 --approval ask     # press y when it pauses
```

Then swap segment 6 for the equivalent window of your take (the gate appears
~29 s in). Nothing else changes.

## 4. Honesty caveats — do not narrate past these

1. **The two runs in the footage are n=1 each, not the benchmark.** On this
   pair the baseline used 15 tool calls and 846 k input tokens against
   Spyglass's 11 and 241 k — a 3.5× gap. That is a vivid illustration and it is
   **not the result**. The committed benchmark (n=3) says S1 is a **tie on
   correctness**, and that Spyglass is on average slightly *dearer* on S1
   (461 k vs 424 k). One baseline run happening to burn 846 k is variance, not
   a finding. If you quote numbers, quote the card at **1:33** — it is
   generated from the 36 committed runs.
2. **The gate really blocked, but the `y` came from a script** (see §3b) —
   do not say "and I approve it" over footage where you did not.
3. **The baseline's `E1`–`E4` are the model's own invention**, not a Spyglass
   failure — that is precisely the point: with raw tools there is no evidence
   id to cite, so a citation cannot be checked. Say that, don't imply the
   baseline was sabotaged.
4. **The session pages are real**, captured from the live TrueForge UI —
   `…/sessions/01m191yh8x7y1pz4awc178wkwp` (baseline, segment 2) and
   `…/sessions/01m191k06bhgskgabb9z7pkj60` (Spyglass, segments 5 and 7). The
   pans are a full-page render scrolled in post, not a hand-scroll. The
   proposal id on the page (`eef65b46…`) is the one shown at the gate.
5. **S6 is a loss for the full system** and the card says so. Keep it.

## 5. The narration, timed to this cut

**≈ 290 words ≈ 1:56 at 150 wpm.** Beat markers match the segment table.

> **[0:00]** Friday afternoon. A payments deploy goes out. Within ten seconds,
> one checkout in five is failing, and the alert fires.
>
> **[0:12]** Give the same model raw tools and it does find it. This is its
> report: raw Prometheus counters, raw journal JSON, a raw stack trace — pasted
> in and labelled E1 through E4.
>
> **[0:21]** Then it asks to roll back production, citing that evidence. Every
> id comes back: *not issued by the engine in this investigation*. The model
> invented them. There is nothing to check.
>
> **[0:29]** So don't make the agent smarter. Make the evidence better.
>
> **[0:39]** Same fault, same model — now reading a Rust evidence plane. It
> names the root cause down to the function, then stops guessing: it takes the
> request a real client sent and replays it twenty times against each live
> version. Version one, zero failures. Version two, twenty out of twenty. An
> experiment, not a correlation.
>
> **[1:01]** The same gate, minutes apart. This time every id resolves — the
> bundle, the exemplar, the replay. The human approves evidence, not a hunch.
> And the gate genuinely waits.
>
> **[1:19]** It also *rejects* the decoy deploy, on timing. Then the engine —
> not the model — verifies recovery: two clean checks, incident closed. Every
> id is a ledger line with a re-runnable digest.
>
> **[1:33]** Thirty-six runs, every one committed. Where the cause is in the
> telemetry, raw tools find it too — a tie. Where there is nothing to find, my
> full system refused correctly once in three. Take novelty away, and it
> refused three times out of three. More evidence made the agent act. That is a
> negative result about my own thesis, and my own benchmark is how I know it.
>
> **[1:50]** Spyglass. `just demo`. The repo has every run.

**Delivery:** stop hard after *"There is nothing to check."* and let 0:27–0:29
sit in silence. Slow down for *"Version one, zero failures. Version two, twenty
out of twenty."*

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
