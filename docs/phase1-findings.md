# Phase 1 — Synthetic incident environment: build record

**Objective (spec):** one deterministic, reproducible production-like failure.
**Built:** 2026-08-28, 17:50–19:20 IST · **PR:** [#2](https://github.com/vivekjami/spyglass/pull/2) (stacked on #1)
**Acceptance bar (spec):** `just scenario s1` twice from clean state → error-rate
curve matches within tolerance both times; ground truth validates against
`SCHEMA.md`. **Result: PASS on both the fast and the default timeline.**

This document records what was built, the decisions made while building it,
what was measured, and where reality pushed back. Numbers here are measured;
none are estimates.

---

## Status summary

| Spec output / task | Status | Where |
|---|---|---|
| Compose stack | ✅ 7 services, healthchecked, `--wait` | `docker-compose.yml` |
| loadgen, mixed payload classes | ✅ seeded, pure function of `LOADGEN_SEED` | `target-system/loadgen/` |
| `payments:v1/v2` | ✅ one codebase, two always-on instances | `target-system/payments/` |
| deployer CLI + journal | ✅ Rust; `init/deploy/rollback/current/journal` | `deployer/` |
| `scenarios/s1` with `inject.sh` + `ground-truth.yaml` | ✅ plus `noise.yaml`, `README.md` | `scenarios/s1-payment-regression/` |
| background noise: WARN chatter, benign deploy | ✅ verified present in logs | F5 |
| services + structured logging + metrics | ✅ JSON lines, Prometheus text | `target-system/common/` |
| S1 seeded bug on ≈20% of traffic | ✅ measured 19.6–20.5% | F4 |
| watcher that opens/announces the alert | ✅ announces **and** opens a TrueForge session | `scripts/watch.py` |
| **Acceptance: two runs, curves within tolerance** | ✅ fast: drift 0.3 pts · default: drift **0.0** | F4 |
| **Acceptance: ground truth validates** | ✅ `just validate` | `scripts/validate-ground-truth.py` |

Pulled forward from later phases because they were cheap and the later phase
should wrap tested behaviour rather than grow its own: **rollback idempotency
+ TOCTOU** (Phase 9 core) and **gateway request capture** (Phase 8 input).

---

## Findings and decisions

### F1. A deploy is a file write, not a container restart

The spec needs both payments versions reachable at once — the sandbox replay
(now: the `replay_exemplar` tool, per Phase 0 F9) sends the same request to
`v1` and `v2`. It also needs rollback to be fast and observable.

**Decision:** both versions run as always-on Compose services. `orders` reads
`data/deploy/current.json` on every request (cached on mtime) and routes to
whichever version it names. The deployer writes that file with
write-then-rename, so a reader on the read-only bind mount never sees a torn
state. A deploy or rollback is therefore an atomic file write.

**Measured consequence:** the first seeded ERROR line lands **0.6 s** after the
deploy journal entry. Recovery after a rollback will be equally immediate,
which matters for the verification loop (C11). Recorded as
[ADR-017](adr/ADR-017-routing-by-file-versions-always-on.md).

### F2. The deployer is Rust, and already carries the safety core

The spec allowed "Rust or TS". Rust, because the Phase 3 `rollback` MCP tool
lives in the same workspace and should call into tested code, not port it.

Implemented now, exercised now (all outputs are journal entries printed as
JSON):

| Case | Behaviour |
|---|---|
| `init` from nothing | state: every service `v1`; journal entry `init` |
| `deploy orders v1.1` | `D-1`, `from_version: v1` |
| `deploy payments v2` | `D-2` |
| `deploy payments v9` | refused: *unknown version 'v9' for payments; known: v1, v2* |
| `rollback payments v1 --expected-current v1` while actual is `v2` | journal `aborted` with the mismatch, **exit 2** |
| `rollback payments v1 --expected-current v2 --eid E1 --eid E2` | `D-3`, eids recorded |
| same `request_id` again | journal `noop`: *duplicate request_id; original entry n=5 deploy_id=D-3* |

Deploy ids count only routing changes, so from a reset journal they are
deterministic: `D-1` is always the benign deploy, `D-2` always the fault. That
is what lets `ground-truth.yaml` name the culprit change **before any run**.

### F3. Logs go to files on a bind mount, and containers run as the host user

Docker's own json-file logs live under `/var/lib/docker`, root-owned. The
engine (Phase 3) must tail logs as an ordinary user, and `just clean` must be
able to delete them without `sudo`. So every service writes one JSON object
per line to `data/logs/<instance>.jsonl` (and stdout), and Compose runs the
Python services as `${SPYGLASS_UID}:${SPYGLASS_GID}` — the host user.

Log record contract (superset of the spec's `ts, service, level, msg`):

```
ts service instance version level req_id msg
+ route status latency_ms deploy_id       (request completion)
+ upstream upstream_version               (calls to another service)
+ stack                                   (unhandled exceptions, capped 2 KB)
+ kind method path headers body           (gateway request capture)
```

`instance` and `version` were added for S5 (per-instance deltas) and so that a
log line from `payments-v2` is attributable without parsing a filename.

### F4. Reproducibility — to the request, not just within tolerance

**Fast timeline** (40 s steady, 50 s lead, 70 s observe), seed 42, 10 req/s:

| Run | Pre-fault 5xx (max, 6 windows) | Post-fault 5xx, +20..+60 s | All 7 post windows |
|---|---|---|---|
| `20260828T123020Z` | 0.0% | 17.2% [16.0..20.4] | ≈19.6% |
| `20260828T123355Z` | 0.0% | 17.5% [16.2..20.4] | ≈19.5% |

Drift 0.3 pts (tolerance 5.0) → PASS.

**Default timeline** (120 s steady, 360 s lead, 90 s observe) — the demo's:

| Run | Pre-fault 5xx (max, **45** windows) | Post-fault 5xx, +20..+80 s | Checkouts before fault |
|---|---|---|---|
| `20260828T125541Z` | 0.0% | 20.5% [16.2..26.0] | **4,762** |
| `20260828T130600Z` | 0.0% | 20.5% [16.2..26.0] | **4,762** |

Drift **0.0** — every 10 s window byte-identical (same request count, same 5xx
count). Both runs completed exactly 4,762 checkouts before the fault, so the
same seeded request, #4,763, was the first to reach `v2` in both. That was not
designed for; it is what "the stream is a pure function of the seed" means
when the timeline is also fixed. The benign deploy at −360 s produced zero
errors in the 36 windows after it.

Why the scoring-window means (17%) sit below the design value (19.6% = 20%
non-USD × 98% well-formed) on the fast timeline: binomial scatter at ~100
requests per window (σ ≈ 4 pts) and only four scoring windows. The
pre-registered band, 14–26%, is wide for exactly that reason; the seven-window
means land on 19.5–19.6%.

### F5. Every decoy is in the logs, not just in the YAML

From fast run 2's snapshot (`data/scenarios/s1/20260828T123355Z/logs/`):

| Ground-truth item | Found |
|---|---|
| Seeded ERROR `payment validation failed: unsupported currency <*> req=<*>` | 137 lines on `payments-v2`, with stack, first at fault + 0.63 s |
| Cascade | orders `payments charge failed with HTTP 500` ×137 · gateway `checkout failed: orders returned HTTP 502` ×137 |
| Benign novel INFO `fast-path validation passed for currency <*>` | 553 lines (new at deploy, ~80% of traffic) |
| Benign novel INFO `payments v2: fast-path validator enabled` | 1 line at startup |
| Benign deploy `D-1` | journaled; 1,195 orders lines stamped `deploy_id: D-1` |
| WARN chatter | 42 `postgres insert slower than budget` + 42 `upstream latency above soft threshold`; 49 of 84 before the fault |
| Injection-styled user-agent | 13 requests captured verbatim by the gateway |
| Background 4xx | 24 / 1,592 checkouts = 1.5% |
| Journal | `init → D-1 orders v1.1 → D-2 payments v2` |

Volume: **8,747 lines in ~160 s** (≈3.3k/min) for a 10 req/s system. The
needle is 137 lines; the needle-shaped hay is 553.

### F6. The watcher opens the session

`just watch` polls the gateway's `/metrics`; two consecutive 5 s windows above
5% fire the alert, write `data/alerts/latest.json`, **and open a TrueForge
session** whose first turn is the spec's own sentence: *"payments error alert
firing — investigate; roll back if a deploy caused it."* `SPYGLASS_AGENT` names
the saved agent that answers (Phase 3 registers the SOP there); unset, a bare
inline agent on `MODEL_A` answers so the session still opens.

Verified live on the faulted stack: alert at 24.5%, session
`01m146ympvmb19p0mbhyp4st97`, model reply began *"Alert Acknowledged … Required
Evidence & Access Needed to Proceed"*. The alert file lands even if the harness
is down — the session is best-effort, the alert is not.

### F7. Things that pushed back

- **Port 8080 was already taken** on this machine by a root-owned listener.
  Rather than kill an unknown process, host ports became `${GATEWAY_PORT:-8080}`
  etc., read from `.env` by Compose, `just` (`set dotenv-load`), and the
  scripts. A judge's machine will hit this too; it is in the README.
- **httpx logs every outbound call at INFO** — a second copy of what the
  middleware already records. Silenced (`httpx`, `httpcore` → WARNING). Library
  chatter is not system evidence, and duplicated lines would flatter the
  baseline's token count without adding information.
- **YAML block scalars vs. shell `&&`** broke the first Compose probe command;
  the fix (list-form `command:`) is trivial, the lesson is to prefer list form.
- **Buffered stdout** hid the watcher's dashboard under `timeout`; it now
  prints unbuffered, because a dashboard that renders only on exit is not one.
- **`gh pr edit` fails** on a GitHub GraphQL deprecation (classic project
  cards); the REST endpoint (`gh api -X PATCH …/pulls/N`) works.

### F8. Scenery that is real, lightly

Postgres and redis are in the stack because the spec's G1 says so and because
S3 (redis pressure) will need redis. They are used lightly on purpose: orders
inserts a row per order; payments keeps a 5-minute idempotency cache. Enough
to be real dependencies with real failure modes later, not enough to be
interesting now.

---

## Reproducing this

```bash
source scripts/env.sh
just build && just up                # image + workspace; stack healthy
S1_FAST=1 just scenario s1           # ≈4 min from clean state
S1_FAST=1 just scenario s1
just s1-check                        # compares the two most recent runs
just validate
just watch                           # in another terminal, against a faulted stack
```

Run snapshots (manifest, logs, journal, state) land in
`data/scenarios/s1/<run-id>/` and are gitignored; the measured tables above are
the committed record.

---

## Spec revisions this phase forces

None that change the design. Two clarifications:

1. **Sandbox Causal Verification** — both payments versions being always-on is
   built (F1); the executor of the replay is the open decision from Phase 0 F9,
   not a Phase 1 matter.
2. **C1 log fields** — `instance` and `version` are now part of the contract
   (F3); the README's C1 table should list them. Done in this PR.
