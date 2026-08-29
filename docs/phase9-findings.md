# Phase 9 — Approval + remediation, hardened: build record

**Objective (spec):** upgrade Phase 3's crude action path to the full safety
model.
**Built:** 2026-08-29, 15:55–17:10 IST (10:25–11:40 UTC) · **PR:** #9
**Acceptance bar (spec):** double-fire test → one rollback + one recorded
no-op; approve-after-manual-rollback test → deployer aborts on version
mismatch; S1 closes only after two clean verification checks.

---

## Status summary

| Spec task | Status | Where |
|---|---|---|
| Idempotency keys | ✅ **system-minted**: `propose_rollback` records a `proposal` (no change) with a v4 `proposal_id`; the gated `rollback(proposal_id, …)` consumes it; a repeat is a journaled `noop`. The model never supplies the key (P2 F7 closed) | `deployer::propose`, `deployer::execute` |
| TOCTOU current-version check | ✅ the proposal snapshots `expected_current`; execution re-checks the live version and journals `aborted: version mismatch` — no `D-n` minted | `deployer::execute` → `rollback` |
| Approval-timeout behaviour | ✅ proposals expire (`expires_at`, 600 s default; `--proposal-ttl-secs`); an expired proposal is refused and journaled. **The harness gate never times out** (F3) — the deployer's clock is the only one | `deployer::execute` |
| Verification loop with escalation paths | ✅ engine-judged: `verify_recovery(service, deploy_id)` — three windows resolved from the journal; clean = post ≤ max(1.5 × baseline, baseline + 2 pt); two consecutive clean checks ≥ 15 s apart close (`verified_recovery` ledger entry); worsening / rising / 5-minute timeout escalate (`escalation` entry, terminal); `too_soon` and `insufficient_data` are not verdicts | `crates/spyglass-engine/src/verify.rs`, `spyglass.toml [verify]` |
| Justification eids rendered at the gate | ✅ `rollback` restates service / version / `expected_current` / eids — refused if they differ from the proposal; the runner resolves each eid to the ledger line that issued it and prints that at the gate | `scripts/investigate.py::render_gate` |
| Engine-side backstop (Safety Model) | ✅ `[limits]`: 200 calls per investigation, 60 per minute, refused with "synthesise from what you have" | `Investigation::admit` |
| **Acceptance: double-fire** | ✅ unit + live: `executed D-n`, then `noop: duplicate proposal_id`; journal `proposal, rollback, noop` (F1) | `deployer` tests; `just s9-check` |
| **Acceptance: approve after manual rollback** | ✅ unit + live: `aborted: version mismatch: proposal expected current=v2, actual current=v1`; nothing changes, no `D-n` (F1) | 〃 |
| **Acceptance: S1 closes only after two clean checks** | ✅ three agent runs closed by the engine, never by the agent; the record run's checks 20 s apart with a `too_soon` in between not counted (F4, F5) | `bench/results/`, ledger `verified_recovery` |
| Deny path | ✅ approval refused → report-only, zero further tool calls, no retry (F6) | `bench/results/s1-spyglass-20260829T104656Z.json` |

---

## Findings and decisions

### F1. Acceptance, measured live

`just s9-check` on a fresh S1 fault (run `20260829T110809Z`):

```
VERIFY   proposal 4bd66d36 → rollback executed D-3
  check 1: insufficient_data (post 0.0% over 0 req; baseline 0.0%, tol 2.0%; incident 19.8%)   → sleep 15 s and call again
  check 1 (not counted): too_soon (183 req) -- 5 s since the last check, checks must be 15 s apart
  check 2: clean (1/2) (604 req)      check 3: recovered (1072 req) → CLOSED
  ledger: freshness_watermark, verify_recovery ×4, verified_recovery -- "recovery verified by 2 consecutive clean checks (3 checks in all)"
ESCALATE fault re-introduced after the fix (D-3); 66 s later
  check 1: worsening (post 23.7% over 1788 req vs incident 19.8%) → ESCALATE; ledger: verify_recovery, escalation
  a later check: escalated -- "ESCALATED earlier; take no further action" (terminal)
DOUBLE-FIRE  proposal a902cc17 → rollback #1 executed D-5 | rollback #2 noop: duplicate proposal_id; already executed as entry n=8 deploy_id=D-5
             journal kinds added: [proposal, rollback, noop]
TOCTOU       proposal 756961c4 expected_current v2; operator deployed v1 (D-7); approved rollback → aborted: version mismatch: proposal expected current=v2, actual current=v1
             journal kinds added: [proposal, deploy, aborted]; no deploy id minted by the abort
EXPIRED      proposal de24131c ttl 1 s → after 2 s → aborted: proposal expired at 11:12:34.359Z; re-propose against the current state; payments still v2
RESTATED     proposal 6de3632a cites [E1, E2]; rollback restating [E9] → aborted: restated proposal differs from the minted one; the faithful restatement → executed D-9
BUDGET       call #61 refused: rate limit: 60 tool calls in the last minute (limit 60); stop and synthesise from the evidence you have
PASS
```

Plus seven deployer unit tests (double-fire, manual-rollback abort, expiry
with the clock rewritten into the past, restated mismatch, unknown
proposal, refused proposals, deterministic `D-n` untouched by proposals)
and nine on the verification decision function.

### F2. The model never mints the key

Phase 2 recorded the model's "fresh UUID": ascending hex nibbles. The fix
is structural, not a better prompt: `propose_rollback` is a non-mutating
tool on the deployer that validates the target, refuses a proposal that
would change nothing or cites no `E<n>`, mints a v4 `proposal_id`,
snapshots the live version, stamps an expiry and journals a `proposal`
entry (which consumes no `D-n`, so ground truth's deterministic ids stand).
The gated `rollback` takes only that id plus a restatement. A key the model
cannot invent cannot collide across incidents.

### F3. The harness gate never times out

TrueForge holds a pending approval in an in-memory map
(`pendingApprovals`) until it is answered; the OpenAPI schema has no expiry
on `tool.approval_required`, and the harness bundle has no approval
timeout. The spec's "an expired approval is never executed" therefore
cannot be the harness's property. It is the deployer's: the proposal
expires, and the check runs where the action runs — tested with a
one-second TTL live and with the clock rewritten in the unit test.
`docs/phase0-findings.md` F4 gains the addendum.

### F4. Recovery is the engine's verdict, and the agent tried to rush it

The spec's C11 loop moved into the engine as `verify_recovery`. It resolves
three windows from the journal — the pre-incident baseline (5 min before
the deploy the action reverted), the incident (that deploy → the action),
the post window (the last 60 s of ingested data after the action, ending
at the safe watermark) — counts request lines, and applies the decision
function (`judge_at`, pure, nine tests): clean within tolerance; two
consecutive clean checks close; a post rate no better than the incident,
rising across two dirty checks, or five minutes without recovery escalate;
too few requests is not a verdict; closed and escalated are terminal.

The first two agent runs exposed a hole: the agent called `verify_recovery`
twice **two seconds apart** and the engine counted both — "two consecutive
clean checks" on the same 500 requests. The engine now refuses to count a
check inside `interval_secs` of the last one (`too_soon`, streak unchanged,
`next` says how long to wait). Run 3 shows it: check 2 at 10:57:22, a call
at :24 not counted, check 3 at :42 → closed. A rule the prompt already
stated ("sleep 15 s") is now a rule the engine enforces — the pattern of
every safety property in this project.

### F5. The agent runs

| Metric | P9 run 3 (record; spacing enforced) | P9 run 2 | P9 run 1 | P9 **deny** | P8 run 2 |
|---|---|---|---|---|---|
| Outcome | **CAUSAL** RCA; replay 0/20 vs 20/20; `propose` → gated `rollback` `D-3`; **engine CLOSED** | same; engine CLOSED | same; engine CLOSED | **CAUSAL** RCA; proposal recorded; **approval denied → report-only, 0 further calls** | CAUSAL RCA; rollback; agent-judged verification |
| Verification checks | 4 (insufficient → clean → `too_soon` → recovered), checks 20 s apart | 3 (checks 2 s apart — F4) | 3 (2 s apart — F4) | none (no action) | 2 `error_delta` |
| Tool calls | 14 | 12 | 12 | 8 | 12 |
| Model calls | 15 | 12 | 13 | 8 | 13 |
| Input tokens (cache reads) | 318,993 (235,404) | 251,009 (178,621) | 267,159 (186,756) | 145,914 (105,528) | 260,172 (161,963) |
| Uncached input | 83,589 | 72,388 | 80,403 | 40,386 | 98,209 |
| Peak context | 30.3k | 27.6k | 28.2k | 21.9k | 26.9k |
| Tool bytes → context | 43,638 | 38,386 | 38,313 | 25,464 | 34,333 |
| Wall | 74.6 s | 55.5 s | 63.5 s | 35.3 s | 77.6 s |
| Eids cited | 11 of 12 | 12 of 12 | 13 of 13 | 8 of 8 | 15 of 15 |
| Journal kinds added | proposal, rollback | proposal, rollback | proposal, rollback | proposal | rollback |
| Ledger re-check | PASS 2/2 | PASS 2/2 | PASS 2/2 | PASS 2/2 | PASS 4/4 |

What the gate showed in every run (`investigate.py`), each eid resolved to
its ledger line:

```
*** APPROVAL REQUIRED: rollback
    proposal_id      5a35e3f7-d3ce-4fa6-be42-6d694adc23c6
    service          payments
    to_version       v1
    expected_current v2
    justification    ['E1', 'E2', 'E3', 'E6', 'E7', 'E8']
      E1   build_evidence_bundle: … top: T payment validation failed: unsupported currency <*> req=<*> [ERROR] 1.00 …
      E6   get_exemplar_request: payments-v2:17 → req d49edd91 POST /checkout body 79 B; origin payments-v2 500
      E7   replay_exemplar: req d49edd91 (T21) payments: v1 0/20, v2 20/20 → separated (Δ 1.00)
```

Run 3's postmortem: *"Check 1 (E9) … `insufficient_data` · Check 2 (E10)
… `clean` (1/2) … Recovery changepoint detected at 10:57:00Z · Check 3
(E12) … `recovered` (2/2) · Incident State: CLOSED [E12]"* — the recovery
changepoint the engine reports from `detect_changepoints(baseline = the
incident)` landed and was cited. The cost of the hardening over Phase 8:
two more tool calls (`propose_rollback`, the extra `verify_recovery`) and
one or two more model calls; uncached input tokens are flat (72–84k vs
80–98k). The uncached figure is the one to watch — the total grows with
every model call that re-reads a cached context.

### F6. The deny path is terminal

With `--approval deny` the harness returned *"User denied tool call:
Operator declined: report-only run."* as a tool error; the agent made zero
further calls, wrote the report-only postmortem with the causal check
intact and the action withheld, and left the fault in place — the runner's
post-run rate went 20.8 % → 24.7 %, which is the correct outcome of a
refusal. The proposal stayed in the journal as the record of what was
asked. The SOP's rule and the harness's deny reason did this together; the
deployer never saw a call.

### F7. Things that pushed back

- **The polluted baseline.** The first ordering of `just s9-check` ran the
  deployer tests (four v2 stints) before the verification tests, so the
  "pre-incident baseline" already contained faults: 10 % baseline, 15 %
  tolerance, and a fault-back post rate of 6.9 % read as *clean*. The
  engine was right by its definition — "recovered" means back to the
  baseline, whatever the baseline was. The check now runs verification
  first, on the scenario's clean baseline, and waits for the post window to
  turn fully faulty before expecting `worsening`. Recorded because it is a
  real property: on a system that was already bad, the engine will close
  an incident at "as bad as before", and the postmortem's rates say so.
- **"Rate worsens" needed a definition.** A single dirty check after a
  clean one is a wobble, not an escalation; a dirty check that is worse
  than the previous dirty check by more than the tolerance is. Equal to
  the incident is "no better than the incident" → escalate now; below it
  but above tolerance → wait for the five-minute timeout. Nine unit tests
  pin this.
- **The recovery changepoint** does not always land: the explicit-baseline
  detector needs six buckets of incident to judge against, and the check's
  synthetic incidents are seconds long. In the agent runs (2–3-minute
  incidents) it landed and was cited; it is reported, never required.

---

## Reproducing this

```bash
cargo test --release -p deployer -p spyglass-engine   # 7 + 45 tests
just build && just mcp-up && just tf-setup
S1_FAST=1 just scenario s1 && just s9-check           # on the fresh fault: verify, escalate, double-fire, TOCTOU, expiry, restated, budget
DEMO_APPROVAL=allow just demo                         # SOP v6: propose → gate → engine-judged close
DEMO_APPROVAL=deny  just demo                         # the report-only exit
```

---

## Spec revisions this phase forces

1. **The mutating tool's signature**: `propose_rollback(service, to_version,
   justification_eids)` → `rollback(proposal_id, service, to_version,
   expected_current, justification_eids)`; the restatement is the gate's
   rendering and is verified against the proposal.
2. **"Approval gates expire (TrueForge/gate timeout)"** becomes "proposals
   expire; the harness gate does not" (F3).
3. **C11 is engine-judged**: `verify_recovery` with `[verify]` config;
   checks must be `interval_secs` apart to count; `detect_changepoints
   (recovery=true)` runs inside it; closure and escalation are ledger
   entries the benchmark reads.
4. **The runaway-agent backstop** is `[limits]` on the engine, refused
   with an instruction, measured.
5. **The SOP's exits**: a denial or an escalation is terminal — one
   re-proposal after an `aborted`, never a second action after either.
