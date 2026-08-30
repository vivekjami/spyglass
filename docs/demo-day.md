# Demo day — from a cold machine to a 3-minute video

Everything needed to set Spyglass up, film it, and cut it to under three
minutes. [`demo.md`](demo.md) is the *shot list and why each segment exists*;
this file is the *operator's runbook* — the commands, in order, with what to
expect and what to do when a take goes wrong.

**The one-sentence pitch, for when someone asks:** *incident investigation is
an evidence problem before it is a reasoning problem, so Spyglass shapes the
evidence instead of the model — and the benchmark says where that works and
where it backfires.*

---

## 0. The single most important thing on this page

**Do not film `just demo`.** It runs two things back to back:

| `just demo` step | Duration | Filmable? |
|---|---|---|
| `S1_FAST=1 just scenario s1` — clean, boot the stack, inject the fault | **~160 s** | No. It is a progress log and a wall of container health lines |
| the investigation | **~72 s** | **Yes. This is the film** |

160 of those 232 seconds are setup. Your whole video is 180 seconds. So:
**inject the fault off camera, then record only the investigation.**

```bash
S1_FAST=1 just scenario s1                                   # off camera, ~3 min
just investigate spyglass --scenario s1 --approval ask       # THIS is what you record
```

`--approval ask` is what makes the human gate real on camera: the run pauses
and waits for you to type `y`. (`just demo` passes `ask` too, but bundles the
160 s you do not want.)

---

## 1. Time budget

| | Cold machine | This machine (already set up) |
|---|---|---|
| Setup + verification | ~35 min | ~4 min |
| Rehearsal (one full dry run) | 15 min | 15 min |
| Recording (4 captures, 2 takes each) | 30 min | 30 min |
| Editing + voiceover | 40 min | 40 min |
| Upload + submission form | 10 min | 10 min |

Budget **2 hours 10 minutes** cold, **1 hour 40** warm. The internal deadline
is 22:00 IST; the hard one is 00:30 IST Monday.

---

## 2. Setup

### 2a. If this is the machine the build ran on (warm start)

```bash
cd ~/spyglass
source scripts/env.sh                 # node 22 on PATH, TRUEFORGE_URL
scripts/trueforge.sh status           # want: "running on :8790"; else `start`
scripts/mcp.sh status                 # want: engine/deployer/rawtools/ablation all up
just up                               # stack healthy, all services v1
just tf-setup                         # re-register agents (idempotent)
```

Skip to [§3](#3-recording-setup).

### 2b. Cold machine, from nothing

**Clone into your home directory.** If Docker came from snap — `docker info`
mentions `/var/snap/docker` — it *cannot bind-mount paths outside `$HOME`*.
The mount silently becomes an empty root-owned directory instead of failing,
the services log to stdout only, and `just scenario` dies on
`cp: cannot stat 'data/logs/*.jsonl'`. Nothing else will hint at the cause.
(Phase 11 F3.)

```bash
git clone https://github.com/vivekjami/spyglass.git ~/spyglass     # NOT /tmp, NOT /opt
cd ~/spyglass
```

Prerequisites, in order. The first three install without root:

```bash
scripts/install-node22.sh        # TrueForge needs Node >= 22.14; Ubuntu ships 20
scripts/install-sandbox-deps.sh  # bwrap + socat + rg for the local sandbox
scripts/install-just.sh          # the task runner every command below uses
source scripts/env.sh
```

**The one root step** — the installer prints it, and skipping it breaks the
sandbox silently:

```bash
sudo install -m 0755 ~/.local/bin/socat /usr/local/bin/socat
```

TrueForge starts the sandbox's proxy bridge *inside* the sandbox, whose read
policy allows `/usr`, `/bin`, `/lib`, `/etc` — **not `$HOME`**. A `socat` in
`~/.local/bin` passes the harness's start-up check and is then unreadable by
the bridge, so nothing listens on port 3128 and every sandboxed command dies
at bootstrap with `pip install pydantic … Cannot connect to proxy`
(Phase 11 F1). The demo does not depend on the sandbox, but the harness logs
look alarming without this.

Also needed, from your distro or rustup:

| | Why | How |
|---|---|---|
| Rust ≥ 1.94 | the engine, deployer, raw-tools servers | `rustup` — Ubuntu 24.04 packages 1.75, too old |
| Docker + Compose | the target system | distro package |
| Python 3.12 + PyYAML | scenario and bench tooling | distro package |

Then configuration and build:

```bash
cp .env.example .env
# edit .env: MODEL_PROVIDER, MODEL_API_KEY (a PAID tier -- a free-tier key was
# measured at 20 requests/day, which is not enough for one investigation),
# and GATEWAY_PORT if 8080 is taken on your machine (it usually is)

scripts/trueforge.sh start       # harness on :8790, state in .local/ (disposable)
just build                       # cargo release + the Compose image -- ~3 min cold
just up                          # stack healthy, all services v1
just mcp-up                      # engine :8791, deployer :8792, rawtools :8793, ablation :8794
just tf-setup                    # registers 4 MCP servers + 3 agents
```

### 2c. Verify before you record

Do not discover a broken stack halfway through a take.

```bash
scripts/trueforge.sh status                    # "running on :8790" + sandbox block
scripts/mcp.sh status                          # four servers up
docker compose ps                              # eight containers, all healthy
./target/release/deployer --data-dir data/deploy current    # every service v1
```

Then **one full dry run**, which is also your rehearsal:

```bash
S1_FAST=1 just scenario s1
just investigate spyglass --scenario s1 --approval ask
```

You want to see, in order: the session URL, `turn 1` with ~8 tool calls, the
`*** APPROVAL REQUIRED` block, your `y`, then `verify_recovery` closing the
incident and a `ledger re-check … PASS` line. If that works, everything in
this document works.

---

## 3. Recording setup

**Nothing is installed on this machine.** Two paths:

### Path A — recommended: OBS + ffmpeg

```bash
sudo apt install -y obs-studio ffmpeg
```

OBS gives you microphone audio, scene switching and a visible recording
indicator; ffmpeg gives you the 8× speed-up and the final concatenation.
On Wayland, OBS's screen capture uses the portal — pick **Screen Capture
(PipeWire)** and approve the dialog once.

### Path B — zero install: GNOME's built-in recorder

`Ctrl` + `Shift` + `Alt` + `R` starts and stops recording; a red dot shows in
the top bar. Files land in `~/Videos/Screencasts/` as WebM. GNOME 46 imposes
no length limit (there is no `max-screencast-length` key on this system).

It records **video only** — no microphone. That is fine: the plan records the
voiceover separately anyway. But you will still want `ffmpeg` to assemble:

```bash
sudo apt install -y ffmpeg
```

### Screen and terminal

| Setting | Value | Why |
|---|---|---|
| Resolution | 1920×1080 | what judges will watch it at |
| Terminal font | **≥ 16 pt**, 18 preferred | tool output has long lines; small type is unreadable after compression |
| Theme | dark, high contrast | the session page is light — the contrast helps the cut |
| Browser zoom | 110–125 % on the session page | the rendered tool results are the star |
| Notifications | **off** (Do Not Disturb) | a Slack toast mid-take costs you the take |

**Window layout** — three terminals and a browser:

```
┌─────────────────────────────┬──────────────────┐
│  A: TrueForge session page  │  B: the runner   │
│     (browser, ~60% width)   │  (just investigate)
│     the star of the film    ├──────────────────┤
│                             │  C: just watch   │
└─────────────────────────────┴──────────────────┘
```

**Do not show on camera:** `.env`, `cat .env`, or any terminal where you typed
the API key. Session URLs, ports and ledger ids are all fine — they are local.

---

## 4. The four captures

Record each **twice**. Keep every take; deciding later is cheaper than
re-shooting.

### Capture 1 — the incident begins → video 0:00–0:10

Needs a clean stack (everything on v1).

```bash
just up                       # if anything was rolled back
just watch                    # terminal C -- 0.0% 5xx, payments=v1, green
# terminal B, once the dashboard is visibly green for ~5 s:
./target/release/deployer --data-dir data/deploy deploy payments v2 --actor deploy-bot
```

Record ~25 s. Within one 5 s window the bar climbs to ~20 % and the alert
fires. **Hold on the moment the green becomes red** — that is the frame.

Reset:
```bash
./target/release/deployer --data-dir data/deploy rollback payments v1 --request-id $(uuidgen)
```

### Capture 2 — the naive agent → video 0:10–0:30, played at 8×

```bash
S1_FAST=1 just scenario s1                                  # off camera, ~3 min
just investigate baseline --scenario s1 --approval ask      # record this
```

Runs ~63 s and uses ~19 tool calls. Record the **whole** run — at 8× it
becomes 8 s, and you need the length to sell "watch the context grow."

What the camera must catch: raw log walls scrolling in the session page, the
tool-call counter climbing in the terminal, and the closing totals line.
Freeze the last frame on the totals.

**Narrate this honestly.** The baseline *solves* S1 — in about a minute, for
~424,000 input tokens. The line is *"it got there — at four hundred thousand
tokens"*, never *"it failed."* The accuracy story belongs to S6, which comes
later in the cut.

### Capture 3 — the Spyglass loop, live → video 0:45–2:45

The centrepiece. One continuous take.

```bash
S1_FAST=1 just scenario s1                                  # off camera, ~3 min
just investigate spyglass --scenario s1 --approval ask      # record this, ~72 s
```

Sequence, with what to do:

1. **The session URL prints** in B. Open it in A immediately — the page fills
   in live as the agent works.
2. **`freshness_watermark` → `build_evidence_bundle`.** Two calls. Hold on
   `coverage`: *events_scanned → items_returned* (the last run: **9,723 events
   → 6 items / 4,763 bytes**). Read your own number off the screen.
3. **`get_evidence`** on the top ERROR template — the stack trace, `first_seen`,
   `instances: ["payments-v2"]`.
4. **`get_exemplar_request`** — the sanitized request a real client sent, its
   `chain` through the services, `outcome.origin_5xx: payments-v2`.
5. **`replay_exemplar`** — `comparison: {"v1": "0/20", "v2": "20/20"}`,
   `verdict: separated`. **The single best frame in the video.**
6. **The gate, in B.** `*** APPROVAL REQUIRED: rollback` with `proposal_id`,
   `service`, `to_version`, `expected_current`, and each cited evidence id
   resolved to its ledger line. **Let it sit for three seconds before you type
   `y`.** That pause is the control-and-safety story.
7. **`verify_recovery` × 3** in A: `insufficient_data` → `clean (1/2)` →
   `recovered … CLOSED`. Terminal C's bar falls to 0 %.
8. **The closing lines** in B: the rollback, `5xx before 21.6% -> after 0.0%`,
   and `ledger re-check: … -> PASS`.

Reset between takes: `S1_FAST=1 just scenario s1` again.

### Capture 3b — the scroll-through (do this immediately after 3)

The live run is ~72 s but fills 120 s of video. After it finishes, **the
session page persists** — record a second pass scrolling slowly back through
it, zooming on each of the six frames above. Cut between live footage (for the
gate and the close) and scroll-through (for the evidence frames).

This is the difference between a rushed segment and a calm one.

### Capture 4 — the numbers → video 2:45–3:00

```bash
just report        # regenerates the tables from bench/results/ -- no hand-typed numbers
```

Open `README.md` → **Results** (between the `bench-results` markers) at a zoom
where the **Success**, **No wrong action**, **RCA acc.** and **Total tokens**
columns are legible. Show the S1–S3 rows, then **the S6 rows** — that is the
interesting result.

Optional B-roll, 3 s each: `ls bench/results | wc -l` (**53** — the 36
benchmark runs plus every phase-development run, all committed, failures
included) and `just bench --dry-run` (the 36-cell plan). The honesty is
visible as a directory listing.

End card: repo URL · `just demo` · the one-line thesis.

---

## 5. The cut, and the script

**Both live in [`presentation.md`](presentation.md)** — §4 is the
second-by-second cut (2:55, with which capture feeds each segment and why the
segment lengths are what they are) and §5 is the narration word for word, with
delivery notes. That file is the single source of truth for content; this one
stays the mechanics.

Two things from it worth repeating here, because they change how you *shoot*:

- **Segment 6 (the gate, 1:42–2:07) must come from the live take**, not the
  scroll-through — the keystroke has to be real.
- **Segments 4, 5 and 7 come from the scroll-through** (Capture 3b), because
  72 s of run has to fill ~75 s of evidence footage and you want to linger.

## 6. The three slide cards

The video has three slides; everything else is screen capture. They are built
in [`deck/index.html`](deck/index.html):

```bash
xdg-open docs/deck/index.html      # or open it in Chrome/Firefox
```

- `f` — full screen
- **`c` — clean mode. Press this before recording.** It hides the slide
  counter, the navigation hint and the "video card" labels, which would
  otherwise be burned into your footage.
- `→` / `←` — navigate · `p` — print to PDF (for a deck handout)

Record each card full-screen for ~5 s longer than you need, and trim. The
three cards are slides **3** (the turn), **10** (the results) and **13** (the
end card); the other ten slides are the full deck for a live round or judge
Q&A — see [`presentation.md`](presentation.md) §3.

## 7. Editing

Assuming the captures are `cap1.webm … cap4.webm` in `~/Videos/Screencasts/`.

```bash
cd ~/Videos/Screencasts

# 1. speed the baseline segment up 8x and drop its audio
ffmpeg -i cap2.webm -filter:v "setpts=PTS/8" -an cap2_8x.mp4

# 2. trim each clip to the length the cut needs (adjust in/out to taste)
ffmpeg -i cap1.webm -ss 00:00:04 -t 10 -c:v libx264 -crf 20 -an c1.mp4
ffmpeg -i cap2_8x.mp4 -t 20 -c copy                                 c2.mp4
ffmpeg -i card.png   -loop 1 -t 15 -vf scale=1920:1080 -pix_fmt yuv420p c3.mp4
ffmpeg -i cap3b.webm -ss 00:00:00 -t 75 -c:v libx264 -crf 20 -an  c4.mp4
ffmpeg -i cap3.webm  -ss 00:01:05 -t 45 -c:v libx264 -crf 20 -an  c5.mp4
ffmpeg -i cap4.webm  -t 13 -c:v libx264 -crf 20 -an               c6.mp4

# 3. concatenate
printf "file c1.mp4\nfile c2.mp4\nfile c3.mp4\nfile c4.mp4\nfile c5.mp4\nfile c6.mp4\n" > list.txt
ffmpeg -f concat -safe 0 -i list.txt -c copy silent.mp4

# 4. lay the voiceover over it (record vo.wav in OBS, mic only)
ffmpeg -i silent.mp4 -i vo.wav -map 0:v -map 1:a -c:v copy -c:a aac -shortest demo.mp4

# 5. check the length -- it must be under 3:00
ffprobe -v error -show_entries format=duration -of csv=p=0 demo.mp4
```

If the total runs long, cut from segments 4 and 7 (the scroll-throughs), never
from the gate or the numbers.

---

## 8. When a take goes wrong

This is a live LLM against a live system. Runs differ. **Read every number off
the screen and say what it says** — never re-shoot to get a prettier number,
and never type a number into the narration that the take does not show.

| What happened | What to do |
|---|---|
| `replay_exemplar` says **19/20** instead of 20/20 | Keep it. Say "nineteen of twenty." It is still `separated`. |
| `not_separated` on the first exemplar | The SOP tries one other exemplar. If it still fails, the RCA is correlational and honest — but re-shoot: S1 came out `separated` at 0/20 vs 20/20 in **4 of 4** recorded runs, so this is unlucky, not typical. |
| The agent used more calls than last time | Fine. The counter is not part of any claim in the narration. |
| The gate never appeared | You passed `--approval allow`. Re-run with `--approval ask`. |
| The engine **escalated** instead of closing | Happened once in 36 runs (S2, a false escalation — Phase 10 F6b). Re-run the scenario and shoot again; do not present an escalation as a close. |
| The agent rolled back the *wrong* thing | Only ever observed on S6, never on S1. Re-run. |
| `cp: cannot stat 'data/logs/*.jsonl'` | The snap-Docker `$HOME` trap — [§2b](#2b-cold-machine-from-nothing). Re-clone under `$HOME`. |
| A container is unhealthy | `just clean && just up`. If port 8080 is taken, set `GATEWAY_PORT` in `.env`. |
| The harness is unreachable | `scripts/trueforge.sh start`; check `.local/trueforge.log`. |
| Sandbox errors in the harness log | Expected without the socat root step, and harmless — the demo does not use the sandbox (Phase 11 F1). |

---

## 9. Before you upload

- [ ] Duration **under 3:00** (`ffprobe`, above)
- [ ] Audio audible; no clipping; no background noise
- [ ] Terminal text legible at 1080p after compression — check on a phone
- [ ] No API key, no `.env`, no personal information in any frame
- [ ] Every number spoken matches a number visible on screen
- [ ] The baseline segment is labelled as the baseline, not passed off as Spyglass
- [ ] The S6 result is in the cut
- [ ] End card shows the repo URL

---

## 10. After the video

1. **Authorize Qodo Merge** — GitHub → Marketplace →
   [Qodo Merge](https://github.com/apps/qodo-merge-pro). One click, by the
   repository owner.
2. **Add the video link through a pull request**, not a push to `main`. It has
   to go into [`submission.md`](submission.md) and the README anyway, so it is
   the natural last change — and with Qodo authorized it becomes the one PR in
   this repository carrying a real Qodo review, which the README's *Qodo
   status* line can then point at. Every earlier PR merged before Qodo was
   installed, and the README says so plainly.
3. **Submit the form** — <https://forms.gle/PxGLsWW1HPyroQ5u9> — using the
   field table at the top of [`submission.md`](submission.md). Before
   **22:00 IST**; the hard deadline is 00:30 IST Monday.

---

## Appendix — the numbers you may be asked about

All measured, all traceable to committed run files in `bench/results/`.

| Claim | Number | Where |
|---|---|---|
| Bundle reduction (the demo run) | 9,723 events → 6 items / 4,763 B | `bench/results/s1-spyglass-20260830T015120Z.json` |
| Causal replay, S1 | v1 0/20, v2 20/20, `separated` | same |
| Spyglass on S1 (post-Phase-11) | 11 tool calls, 242k input tokens, 71.7 s | same |
| Baseline on S1 (matrix, n=3) | 19 calls, 424k input tokens, 63 s | `docs/benchmark.md` |
| Benchmark size | 36/36 valid runs, 0 failures, 4 h 35 min | `docs/benchmark.md` |
| S1–S3 success, every condition | 9/9 | `docs/benchmark.md` |
| S3 — where shaping pays | 8.7 calls vs 14; 139k vs 210k tokens | `docs/benchmark.md` |
| **S6 — the negative result** | ablation 3/3, Spyglass 1/3, baseline 0/3 | `docs/benchmark.md`, P10 F6 |
| Ledger re-checks | 12/12 Spyglass runs PASS | P10 F6e |
| Sandbox | enabled, called, **never executed** — diagnosed | P11 F1 |

If a judge asks *"did the sandbox do the causal replay?"* — no, and the repo
says so: the harness sandbox is network-isolated and cannot reach the Compose
stack, so the experiment runs on the evidence plane. The agent still designs
it. [ADR-010](adr/ADR-010-sandbox-verification-before-action.md) has the whole
story, amended rather than quietly reversed.
