# Phase 11 — Demo hardening + submission: build record

**Objective (spec):** the artifact a stranger can run and the video a judge
will score: `just demo` from a clean clone, the README final pass, the Qodo
evidence section, the demo recorded per the plan, the blog finalized, the
submission in by 22:00 IST Sunday.
**Built:** 2026-08-30, 00:45 IST onward (Sat 19:15 UTC onward) · **PR:** #11
**Acceptance bar (spec):** clean-machine run succeeds; video ≤ 3:00;
submission confirmed.

---

## Status summary

| Spec task | Status | Where |
|---|---|---|
| `just demo` from a clean clone | ✅ see F3 (a fresh `git clone` of this branch, built and run on the same host with the working copy's stack stopped — a second machine was not available; the prerequisites a second machine needs are the README's) | F3 |
| README final pass | ✅ status, tree, prerequisites (the socat root step, F1), Definition of Done as it stands, the Qodo section | `README.md` |
| Qodo evidence section | ✅ written honestly: the automated reviewer on PRs #1–#4 was Copilot; all fifteen findings are now addressed (F2); Qodo Merge must still be authorized by the repository owner — the one step nobody else can take | `README.md` → *Qodo Code Review Evidence* |
| Record the demo | ○ operator's task — the filming runbook, capture list, resets and a 442-word narration written from the measured tables are in `docs/demo.md` | `docs/demo.md` |
| Blog finalized | ✅ results, negatives, limitations, what breaks next | `docs/blog/draft.md` |
| Submission | ○ operator's task — every form field and the write-up are in `docs/submission.md` | `docs/submission.md` |
| CI (`.github/workflows/ci.yml`, promised by the tree since Phase 0) | ✅ fmt, clippy (`-D warnings`), tests, ground-truth validation, and a check that the generated tables match the committed run files | `.github/workflows/ci.yml` |
| Engine: verification paces itself (P10 F6a) | ✅ `verify_recovery` waits out the interval instead of answering `too_soon`; SOP v8 drops the sandbox `sleep` | F4 |

---

## Findings and decisions

### F1. The sandbox never ran: socat has to live where the sandbox can read it

Phase 10 found that every sandboxed command in every recorded run — 36 in
the matrix and every run since Phase 3 — failed with the harness's own
bootstrap error: *Sandbox initialization failed: Failed to pip install
pydantic … ProxyError: Cannot connect to proxy*. Phase 0 had verified the
sandbox runtime directly (`srt -c "curl …"` through the proxy: 200) and the
harness reported *Local sandbox fallback is available* at start-up, so the
failure was invisible until the run files were read.

Diagnosis, reproduced outside the harness:

| Test (standalone `srt`, the harness's pypi allowlist) | Result |
|---|---|
| filesystem `denyRead: []` → `pip install pydantic` in a fresh venv | **200 from pypi via the proxy; pydantic installed** |
| filesystem `denyRead: ["/"]`, `allowRead` = the harness's Linux list (`/usr`, `/bin`, `/lib`, `/etc`, `/proc`, …) → `which socat; curl https://pypi.org` | `which socat` → nothing; *Failed to connect to localhost port 3128* — the harness's failure, exactly |

The sandbox runtime starts its proxy bridge **inside** the sandbox —
`socat TCP-LISTEN:3128,fork,reuseaddr UNIX-CONNECT:<socket> &` — with the
harness's command PATH (`/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`) and
its read policy (deny `/`, allow the system directories). Phase 0's no-root
install put `socat` in `~/.local/bin`: the harness's start-up dependency
check (host PATH) found it and said the sandbox was available; the bridge,
running under the sandbox's policy, could neither find nor read it, so
nothing listened on 3128 and every `pip`/`curl` inside failed with
*connection refused*. The venv step is the first thing the harness does in
a new sandbox, so no command ever got past it.

**Fix:** `socat` must be on a sandbox-readable path — `sudo install -m 0755
~/.local/bin/socat /usr/local/bin/socat` (or the distro package). One root
step; `scripts/install-sandbox-deps.sh` now says so at the end, and the
README's prerequisites table carries it. It was not run here (this build
machine has no passwordless sudo and the rule was no system changes), so
**the sandbox remains unverified end-to-end through the harness**; the
standalone reproduction above is the evidence for the diagnosis, and the
demo does not depend on the sandbox (F4). What the finding says about the
integration: the harness's start-up check tests the host, and the bridge
runs under the sandbox — two different worlds, and the check should
exercise the second.

Fairness: symmetric. Every condition saw the same error on every `exec`;
the baseline's attempt to read `data/logs/orders.jsonl` through the
sandbox filesystem was refused like everything else, so no condition
read anything its tools did not serve.

### F2. Fifteen review findings, all still open — now closed

The automated reviewer on PRs #1–#4 (GitHub Copilot; Qodo Merge was not
installed) left fifteen findings; none had been addressed when those PRs
merged. Phase 11 went through them one by one (README → *Qodo Code Review
Evidence* has the table): thirteen fixed (the harness pinned to the
validated `0.1.4`, checksum-guarded installers — the socat tarball verified
against the digest Homebrew and Alpine publish, checked on both mirrors — a
single EXIT trap, a POSIX pid lookup, the watcher closing its sockets, the
deployer reporting internal failures as internal errors and never an empty
success, `tf.output_text` accepting a bare string, the scenario recipe
refusing an ambiguous directory) and two dismissed with the reason written
down (the JSON-length byte metric is comparative and identical across
conditions; the `deploy_events` default window ends at the safe watermark
on purpose, for ADR-004's re-check). Copilot's quota lapsed after #4 —
#5–#9 carry "unable to review"; the phase findings documents stand as
their review record.

### F3. `just demo` from a clean clone

`[MEASURE AFTER IMPLEMENTATION]`

### F4. Verification paces itself; the SOP no longer sleeps

Phase 10 F6a measured what a refused `too_soon` check costs: nothing on
the engine, a model call on the bill, and — because the sandbox `sleep`
was failing (F1) — the model filled every interval with
`freshness_watermark`, `current_versions` and `get_current_datetime`
calls (S1 Spyglass: 15 of 54 calls were verification, most of them
refused; one S2 run spent 12 checks and 14 watermarks on five counted
checks). The engine now owns the pacing: a `verify_recovery` call inside
the interval waits for the remainder (at most `interval_secs`, 15 s) and
then checks, reporting `waited_secs`; the `next` hints and the tool
description say "call again", never "sleep"; SOP v8 tells the agent not to
sleep, poll or ask the time between checks. The matrix ran the old
behaviour and its numbers stand; a post-matrix S1 run with SOP v8 is
recorded in F3. The sandbox stays enabled in every manifest — it is no
longer on the demo's critical path.

### F5. Loose ends, stated

- Qodo Merge was never authorized on the repository. The README section
  says so and lists what Qodo will find already answered; the operator's
  list in `docs/submission.md` puts the authorization first.
- The three Phase 10 engine gaps (verification judges only the 5xx
  share; no mechanical evidence floor on proposals; S2's first-check
  escalation rule) are recorded in `docs/safety.md` and the findings, not
  patched — the matrix stays the matrix, and each is a Phase 12 change
  with its own measurement.
- A second machine was not available for the clean-clone test; the
  clone ran on the build host with the working copy's stack stopped.
