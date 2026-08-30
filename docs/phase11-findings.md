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
| `just demo` from a clean clone | ✅ **passed**: clone → `just build` (181 s) → `mcp-up` → `tf-setup` → `just demo` (292 s), gated rollback, engine-closed, 12/12 eids, ledger re-check PASS (F3). Same host, working copy's stack stopped — a second machine was not available. Take 1 failed and found the snap-Docker `$HOME` bind-mount trap, now a documented prerequisite | F3 |
| README final pass | ✅ status, tree, prerequisites (the socat root step, F1), Definition of Done as it stands, the Qodo section | `README.md` |
| Qodo evidence section | ✅ written honestly: the automated reviewer on PRs #1–#4 was Copilot; all fifteen findings are now addressed (F2); Qodo Merge must still be authorized by the repository owner — the one step nobody else can take | `README.md` → *Qodo Code Review Evidence* |
| Record the demo | ○ operator's task — the filming runbook, capture list, resets and a 442-word narration written from the measured tables are in `docs/demo.md` | `docs/demo.md` |
| Blog finalized | ✅ results, negatives, limitations, what breaks next | `docs/blog/draft.md` |
| Submission | ○ operator's task — every form field and the write-up are in `docs/submission.md` | `docs/submission.md` |
| CI (`.github/workflows/ci.yml`, promised by the tree since Phase 0) | ✅ fmt, clippy (`-D warnings`), tests, ground-truth validation, and a check that the generated tables match the committed run files | `.github/workflows/ci.yml` |
| Engine: verification paces itself (P10 F6a) | ✅ `verify_recovery` waits out the interval instead of answering `too_soon`; SOP v8 drops the sandbox `sleep`. Measured: S1 in **11 tool calls / 242k input tokens** against the matrix's 18.3 / 461k | F4, F3 |
| Pre-merge audit of the whole repository | ✅ six readers × two skeptics each; 51 findings, 37 upheld and fixed — including `just s9-check`, which the F4 change had silently broken (re-run live: **PASS** in 116 s) | F5 |

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

### F3. `just demo` from a clean clone — and the mount that silently is not one

Run against `phase-11-demo` at `3ad5c00`, with the working copy's stack torn
down first (the Compose project name is fixed, so only one stack can exist).

**Take 1 failed, and found something worth documenting.** Cloned into
`/tmp/…`, `just build` succeeded (172 s) and the stack came up healthy, but
`inject.sh` died on `cp: cannot stat 'data/logs/*.jsonl'`. The services were
logging — to stdout only. Inside the container `/var/log/spyglass` was owned
by `root` and unwritable, while on the host the same directory was
`vivek:vivek`. Docker here is the **snap** build, and a snap-confined daemon
cannot bind-mount paths outside `$HOME`: the mount silently becomes an empty
root-owned directory instead of failing. Proof, two `docker run --rm`
probes against the same alpine image:

| bind-mounted host path | what the container sees |
|---|---|
| `/tmp/…/clone/data/logs` (host `1000:1000`) | `drwxr-xr-x 0 0` — an empty dir, not the mount |
| `$HOME/spyglass-probe` (host `1000:1000`) | `drwxrwxr-x 1000 1000` — the mount |

The application is not at fault: `_setup_logging` catches the `OSError` and
carries on with stdout, which is the right behaviour for a logger. Nothing
in the repo changed as a result except the prerequisite line — **clone into
your home directory if Docker came from snap** — which is now in the
README's Setup Prerequisites. A judge on Ubuntu with the snap Docker who
clones to `/tmp` or `/opt` would hit exactly this, and the symptom (an
empty `data/logs`) points nowhere near the cause.

**Take 2, cloned into `$HOME`: the whole path passed.**

| Step | Result |
|---|---|
| `git clone` → `just build` | ✅ 181 s from cold (cargo release + the Compose image) |
| `just mcp-up` | ✅ engine :8791, deployer :8792, rawtools :8793, ablation :8794 |
| `just tf-setup` | ✅ 4 MCP servers + 3 agents registered (idempotent) |
| `DEMO_APPROVAL=allow just demo` | ✅ 292 s end to end: fresh S1 (160 s of timeline) → investigation → gate → rollback → engine-judged close → ledger re-check |

The investigation itself (`bench/results/s1-spyglass-20260830T015120Z.json`,
committed; `tag: demo`, `benchmark: false`, so it is outside the matrix and
outside the generated tables): bundle **9,723 events → 6 items / 4,763 B**;
`replay_exemplar` **v1 0/20 vs v2 20/20 → separated (Δ 1.00)**;
`propose_rollback` → gated `rollback` of `D-3` citing E1/E2/E3/E7/E8/E9;
**engine CLOSED** after 3 checks; 5xx **21.6 % → 0.0 %**; **12/12 eids
cited**; ledger re-check PASS. Engine latency p50 10.2 ms.

**And it measured F4.** This is the first S1 run under SOP v8 with the
self-pacing `verify_recovery`, against the matrix's three S1 Spyglass runs
(SOP v7, polling). n=1 against n=3, one scenario, and a demo run rather
than a `--bench` run — a labelled addendum, not a benchmark result, exactly
as F6b said the fix would be reported. **The matrix stays the matrix.**

| | matrix S1 spyglass (n=3, mean) | clean-clone run (n=1) | |
|---|---|---|---|
| tool calls | 18.3 [18..19] | **11** | −40 % |
| of which `verify_recovery` | 5.0 [4..6] | **3** | −40 % |
| `too_soon` responses | 9.0 [6..12] | **0** | the interval is waited out, not refused |
| `freshness_watermark` / clock calls | 2.7 / 1.3 | 1 / 0 | the filler between checks is gone |
| model calls | 19.3 | **12** | −38 % |
| input tokens | 461,480 | **241,951** | −48 % |
| uncached input | 112,402 | **79,540** | −29 % |
| tool bytes → context | 48,636 | 37,288 | −23 % |
| wall (alert → RCA) | 78.3 s | 71.7 s | −8 % |
| sandbox `exec` calls | 1 (the failing `sleep`) | **0** | no sandbox on the critical path |
| outcome | rollback `D-3`, engine CLOSED | identical | |

The mechanism is visible in the run file rather than inferred: the three
`verify_recovery` responses carry `waited_secs` **0, 14, 13** — the engine
holding the call open for the remainder of the 15 s interval — and no
`too_soon` appears anywhere in the trace.

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
behaviour and its numbers stand; the post-matrix S1 run in F3 measures the
change — 11 tool calls against the matrix's 18.3, 242k input tokens against
461k, zero `too_soon`, same verdict and same close. The sandbox stays enabled in every manifest — it is no
longer on the demo's critical path.

### F5. The fix broke its own acceptance test, and only a full audit caught it

F4 made `verify_recovery` wait out the interval instead of answering
`too_soon`. `scripts/gate-check.py` — the Phase 9 acceptance suite behind
`just s9-check`, advertised in the README, `docs/safety.md`, `docs/demo.md`
and `scripts/README.md` — asserted the *old* contract:

```python
if checks[1]["result"]["status"] != "too_soon" or checks[1]["result"]["counted"]:
    fails.append(...)
```

`too_soon` is now unreachable through the MCP surface, which is the only
surface that script uses, so the assertion fired unconditionally and
`just s9-check` exited 3 on a **correctly behaving** system. Nothing caught
it: the engine's own unit test calls `judge_at` directly and so bypasses the
wrapper that does the waiting, and the change's commit touched only
`verify.rs`, `main.rs` and the SOP. A judge running the advertised command
would have seen FAIL.

Two general lessons, both cheap to state and easy to forget:

- **A behaviour change has to be grepped for its assertions, not just its
  callers.** The engine change was correct and tested; what broke was a
  *claim about the old behaviour* living three directories away.
- **A test that asserts a refusal is asserting an implementation detail.**
  The replacement asserts the property that actually matters — the engine
  paced the call — from the response itself: `waited_secs > 0`, `counted`,
  and `since_last_check_secs >= interval - 1`, with the interval read from
  `spyglass.toml` rather than hardcoded, so the check follows the config the
  engine loads.

Re-run live on a fresh S1 fault after the fix: **PASS in 116 s** — closes
after 2 clean checks ≥ 15 s apart (`verified_recovery`), escalates on
`worsening` (`escalation`, terminal), double-fire = 1 rollback + 1 noop,
manual change → `aborted: version mismatch`, expired → `aborted`, restated
mismatch → `aborted` then the faithful restatement executes, and the 61st
call in a minute refused.

It was found by an adversarial audit of the whole repository run before the
merge to `main` — six independent readers (docs-vs-code, numbers-vs-runs,
runnability, secrets, deliverables, coherence), every finding put to two
skeptics instructed to refute it by default. 51 findings, **37 upheld**. The
rest of them were documentation drift the same pass corrected: numbers that
no longer reconciled with the run files (S1–S3 root-cause citation precision
is 86–100 %, not 91–100 %; five of nine Spyglass runs cited a decoy, not
four; 55 S1 tool calls, not 54; A1 ledger mismatches 1–6, not 3–6; 12/12
Spyglass ledger re-checks PASS, not 9/9), stale placeholders the benchmark
had since answered, `SOP v7` labels, an ablation described as a config entry
it outgrew, and three places that still implied the harness sandbox did work
it never did. One finding was refuted on inspection (a "measured number" that
turned out to be measuring a different interval) and was not applied. One I
refuted and was wrong about: an auditor reported a clippy failure at
`crates/spyglass-core/src/lib.rs:635`; `cargo clippy -D warnings` exits 0 on
this machine, so I dismissed it — and CI then failed on exactly that line.
The build host runs clippy **1.94**; the CI toolchain is `stable`, **1.98**,
whose `collapsible_match` covers an `if` inside a match arm. *Clean on my
machine* is a claim about a toolchain, not about the code. Fixed by turning
the arm into a guard (`Value::String(s) if PAN_RE.is_match(s) =>`), which is
identical in behaviour and quiet on both versions — and a reminder that the
CI a reviewer sees, not the laptop, is the arbiter.

### F6. Loose ends, stated

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
- The clean-clone run is n=1 on S1. It is reported as an addendum beside
  the matrix, never folded into it.
