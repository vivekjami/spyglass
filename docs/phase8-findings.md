# Phase 8 — The causal check: build record

**Objective (spec):** the correlation → experiment upgrade, live.
**Built:** 2026-08-29, 15:05–15:55 IST (09:35–10:25 UTC) · **PR:** #8
**Acceptance bar (spec):** S1 replay yields separated proportions (expected
shape ~0/20 vs ~19–20/20 — *measured, not asserted*); results land as ledger
entries.

---

## Status summary

| Spec task | Status | Where |
|---|---|---|
| Request capture in the gateway | ✅ since Phase 1 (`kind=request_capture`: method, path, four-header allowlist, body ≤ 1 kB, auth headers never captured); the engine now indexes captures by request id (F5) | `target-system/gateway/app.py`, `Store::captures` |
| `get_exemplar_request` + sanitization | ✅ by `template_id`, `eid` (a template item → its cited exemplar), `route` + `status`, or `event_id`; sanitized a second time on the way out (auth-shaped headers dropped, secret-shaped body keys and card-like digit runs redacted, values capped — 6 unit tests); returns the request, its `chain` through the services, the 5xx `origin`, and whether it is replayable. Deterministic; re-checks from the ledger | `crates/spyglass-engine/src/replay.rs`, `spyglass-core::sanitize_*` |
| The replay | ✅ `replay_exemplar(exemplar, service, versions, n)`: N per version, straight to the always-on instances' published ports, `replay-*` request ids dropped at ingest; per-version items (k/N, statuses, latency, distinct failure bodies) + `comparison` {proportions, Δ, threshold, verdict, reading}; **one ledger entry, two evidence ids** | `replay.rs`, `spyglass.toml [replay]` |
| Replay pattern in the SOP | ✅ SOP v5 step 4: `get_exemplar_request(eid) → replay_exemplar(…)`; `separated` earns "caused" (for this failure mode), `not_separated` says which way; ACT requires a separated replay when one was possible (ADR-010) | `agent/sop.md` |
| Both payments versions always on | ✅ since Phase 1 (ADR-017); `version = "v1"/"v2"` now declared on the instances so the engine knows the targets | `spyglass.toml [[services]]` |
| Executor | ✅ the evidence engine — Phase 0's fallback (A); ADR-010 expanded and amended | [`ADR-010`](adr/ADR-010-sandbox-verification-before-action.md) |
| **Acceptance: separated proportions, measured** | ✅ **v1 0/20, v2 20/20**, Δ 1.00, `separated`, in 1.0 s — three times out of three (the check and both agent runs); control: a succeeding request 0/20 vs 0/20 `not_separated` (F1) | `just s8-check` |
| **Acceptance: results land as ledger entries** | ✅ entry `n=5 replay_exemplar eids [E7, E8]` in the check, `[E7, E8]` / `[E8, E9]` in the agent runs; the rollback proposals cite them (F6) | `ledger/` |
| Analyst briefs (README tree, "P8") | ✅ three briefs + a conditional fan-out step; **not triggered on S1**, by design (F8) | `agent/subagents/` |
| Baseline fairness | ✅ `http_request` on the rawtools server — one request, like `curl` (F7) | `crates/rawtools-mcp` |

Deferred per spec: multi-exemplar replay classes.

---

## Findings and decisions

### F1. Acceptance, measured

Fast-timeline run `20260829T095913Z`, fault `D-2` at 10:00:43.566, `just
s8-check`:

```
exemplar E6  req d49edd91  captured 10:00:44.125 by gateway  (the exemplar event the cited evidence item carries)
  POST /checkout  headers [content-type, user-agent, x-client-class, x-request-id]
  body {"currency":"EUR","customer":"cust-102","card_class":"standard","amount":65.26}
  sanitization: dropped [] redacted []
  chain [payments-v2 500, orders 500, orders 502, gateway 502, gateway 502]; origin 5xx payments-v2 500 stack=True; edge 502

replay X-ba410bcd33  → payments /charge  n=20  wall 1.00 s  eids [E7, E8]  ledger n=5
  E7 v1 (payments-v1): 0/20 failed   statuses {200: 20}  p50 44.8 ms
  E8 v2 (payments-v2): 20/20 failed  statuses {500: 20}  p50 4.1 ms   500×20 {"error":"UnsupportedCurrency","req_id":"<replay-req-id>"}
  comparison: {v1: 0/20, v2: 20/20}  Δ 1.0 (threshold 0.5)  → SEPARATED
  reading: the same request fails 20/20 on v2 and 0/20 on v1: for this request class the failure is a
           property of v2 -- causal evidence for THIS failure mode, not proof it is the only one;
           raw proportions at N=20, no p-value claimed

control: a request that succeeded (E9, currency USD) replayed the same way → {v1: 0/20, v2: 0/20} → NOT_SEPARATED
clean:   80 requests sent → 100 replay-tagged lines in the raw logs; engine excluded 100;
         payments-v1 requests in the replay window per error_delta: 0
PASS
```

The exemplar is the first non-USD checkout after the deploy — captured by
the gateway 560 ms after `D-2` and 22 ms before `payments-v2` raised — and
its `chain` is the cascade for one request: payments 500 with the stack,
orders 502, gateway 502. The proportions are exactly the spec's expected
shape, and they were the same in both agent runs (F6). The v1 replays take
45 ms each (the simulated processor latency); the v2 replays fail in 4 ms,
before the processor is reached — visible in `latency_ms`, which is the
kind of detail an investigator reads without being told to.

### F2. The executor is the engine (ADR-010, amended)

Phase 0 F9 established that the harness sandbox cannot reach the Compose
network and that there is no supported knob. Fallback (A) shipped: the
agent designs the experiment — which exemplar, which service, which
versions, N — and the engine sends the bytes. The controlled experiment is
unchanged: same input, versions varied, outcome measured. What was lost is
the "agent-written code runs in the sponsor's sandbox" story; the sandbox
keeps its job in the SOP (the verification `sleep`), and the amendment says
so rather than hiding it. A third "experiment" MCP server was rejected
because evidence ids are per engine session: `E7`/`E8` could not land in
the same investigation from another process.

### F3. An experiment leaves footprints in the thing it measures

The engine tails the logs the replay writes to. Left alone, the causal
check would have inflated the error count it was checking, produced a
`requests_total{payments-v1}` changepoint (v1 has no live traffic during the
fault), and dirtied the agent's verification window. Every replay carries
`x-request-id: replay-<experiment>-<version>-<i>` and `x-spyglass-replay`;
the services stamp `replay` on their request lines (the target got a
five-line change and an image rebuild); the tailer drops any line whose
request id starts with `replay-`, counted in
`freshness_watermark.replay_lines_excluded`.

Measured: 80 requests → **100** lines, because a successful v2 charge logs
the `fast-path validation passed` INFO decoy as well as its request line
(the control replay hits that path; the fault replay raises before it).
The engine excluded exactly 100, and `payments-v1` showed zero requests in
the evidence during the replay window. The first version of the check
asserted `excluded == requests sent` and failed on 100 ≠ 80 — the
expectation was wrong, not the exclusion; the check now counts the tagged
lines in the raw files and requires equality with the engine's counter.

What the replay still touches, stated on every result: the instances'
`/metrics` counters (not the detector's input, ADR-007; not the watcher's,
which reads the gateway) and the payments cache (`charge:<req_id>`, TTL
300 s). `docs/safety.md` records the table.

### F4. Sanitize twice; send what was captured

The gateway keeps four headers and never an auth header. The engine strips
auth/cookie/token/session-shaped headers and redacts secret-shaped body
keys (`password`, `token`, `cvv`, `card_number`, … and anything ending in
`token`/`secret`/`password`/`apikey`) and card-like digit runs *again* on
the way out, with unit tests — because the capture allowlist is a property
of this gateway and the tool has to survive a real one. A body with
nothing to redact is returned **byte-identical** (the replay sends what the
client sent); one with a redaction is re-serialised and says so in
`sanitization.body_redactions`, by JSON path. Amounts, order ids and UUIDs
are not mistaken for card numbers (tested).

### F5. Which request, and why it re-checks

`get_exemplar_request` has four selectors, all deterministic:

- `eid` of a template item → the first of the item's own
  `exemplar_event_ids` whose request id was captured. This is the event the
  evidence already cites; the ledger records `event_id`, so the re-check
  needs no window. The SOP uses this path.
- `template_id` or `route` + `status` → the **earliest** matching event in
  the window with a capture, ordered by (ts, event_id) — never by ingest
  order, which interleaves files. The default window is *all ingested
  history* up to the safe watermark: the exemplar of a failure is the first
  request that failed that way, and the earliest match does not move as
  data arrives (the rule `deploy_events` already used). The first cut used
  the tools' usual "last 15 minutes", which on a stack where the fault was
  16 minutes old returned nothing — found in the first live smoke test.
- `event_id` directly.

Two of three runs on the fast timeline chose the *same* request —
`d49edd91`, EUR, `cust-102`, 65.26 — because the request stream is seeded
and the same index met `v2` first (Phase 1 F-notes); the third chose
`aebbf69f`, JPY. Either is a fair exemplar; the proportions did not change.

### F6. The agent runs

Two `just demo`-equivalent runs with SOP v5, unattended approval, fresh
fault each time.

| Metric | P8 run 1 | P8 run 2 | P7 run 2 (bundle-first, no causal check) | Baseline |
|---|---|---|---|---|
| Outcome | completed: **CAUSAL** RCA, `D-3` citing E1 E2 E3 **E6 E7 E8**, 26.3 % → 0.0 % | completed: **CAUSAL** RCA, `D-3` citing E1 E2 E3 **E7 E8 E9**, 20.6 % → 0.0 % | completed: correlational RCA, `D-3` citing E1/E2/E3 | completed: correlational |
| Replay in the run | E7 v1 **0/20**, E8 v2 **20/20**, `separated`, 1.01 s | E8 v1 **0/20**, E9 v2 **20/20**, `separated`, 1.11 s | — | — (never tried `http_request`; it did not have it) |
| Tool calls | **12** (triage 2, get_evidence 1, exemplar 1, replay 1, current_versions 1, rollback 1, verification 5) | **12** (same sequence) | 11 | 19 |
| Model calls | 12 | 13 | 12 | 11 |
| Input tokens (of which cache reads) | 242,957 (162,426) | 260,172 (161,963) | 184,309 (105,400) | 198,106 |
| Uncached input tokens | 80,531 | 98,209 | 78,909 | — |
| Output tokens | 6,234 | 8,424 | 6,421 | 4,978 |
| Peak context | 26.2k | 26.9k | 21.2k | 30.0k |
| Tool bytes → context | 32,929 (exemplar 4,465; replay 3,765) | 34,333 | 26,536 | 57,147 |
| Wall | 60.5 s | 77.6 s | 71.5 s | 39.5 s |
| Evidence ids cited | **13 of 14** (the uncited one is the INFO decoy) | **15 of 15** | 11 of 11 | none exist |
| Ledger re-check | **PASS 4/4** (5 skipped: 3 watermarks, the replay, get_evidence) | **PASS 4/4** | PASS 3/3 | n/a |

The postmortems now carry the section the spec's RCA template asked for.
Run 1: *"**CAUSAL**: Deploy `D-2` introduced `payments` version `v2` …
[E1, E6]"*; *"Verdict: `separated` (Δ = 1.0) [E7, E8]"*; *"Scope Limit:
Confirms causality specifically for this request class (EUR currency
checkout validation failure mode)"*. Run 2: *"A controlled replay
experiment confirmed that this failure mode is isolated to `payments`
`v2` [E8, E9]"*; *"Limit: The replay evaluates a single captured request
class; it proves causality for this specific failure mode rather than
being proof of all possible failure modes."* Both rollback proposals cite
the replay ids at the gate. `D-1` rejected in both on the 50-second
silence and the origin in `payments-v2`.

**The cost, honestly.** Total input tokens rose 32–41 % over Phase 7
(184k → 243k / 260k). Almost all of it is cache: the two extra model calls
re-read a context that was already there, and the *uncached* input moved
2 % (run 1) and 24 % (run 2); peak context grew 5k with the exemplar and
the replay in it; the engine spent ~1 s. Against the baseline's 198k the
runs are now *above* on total input — and the baseline never ran an
experiment. The correctness bar moved from "correlational, labelled" to
"causal, measured", and the token bar moved with it. Phase 10 reports
tokens next to the RCA's label so the two are never compared without each
other (ADR-012 note, F7).

### F7. Fairness: the raw counterpart

The baseline's tool server gained `http_request` — one request per call to
one instance's published port, method, path, body, headers, like `curl -i`
— and its SOP's tool list says what it is for. The baseline *can* test a
version pair if it thinks to; what it does not get is twenty per version,
a threshold, a verdict, and its own traffic kept out of the logs it reads.
That is the treatment. Neither baseline run so far used it (they predate
it); Phase 10's baseline cells will show whether it does.

### F8. Subagents: briefs written, fan-out conditional, not triggered

The README tree promised analyst briefs at P8; `agent/subagents/` holds
three (logs, metrics, change), each one hypothesis, one question list,
budget ≤ 4 calls, findings-with-eids and a verdict. SOP v5 spawns them
only when the bundle leaves a hypothesis unresolved — no deploy in the
window, no error changepoint, or two live hypotheses in different
services. On S1 the bundle's head names the template, the changepoint and
the deploy of one service within a second of each other, so the lead does
not fan out, and neither run did. Phase 7 measured what the bundle
replaced (7.9 + 6.3 + 1.5 kB of the three analysts' answers, in one 6.8 kB
call); a fan-out there would multiply contexts for no new fact. TrueForge
sub-agents inherit the root's tools and have no budget field (Phase 0
F11), so the briefs are text the lead pastes as `input` — recorded as
built, and where S3/S6 will exercise it.

### F9. Things that pushed back

- **`usize` for an HTTP status.** Gemini's schema validator has rejected
  `format: uint16` shapes before; `status` is declared `usize` like every
  other integer argument that has worked, and range-checked in code.
- **Twenty "distinct" failures.** The first replay reported 20 distinct
  failure bodies because each carried its own replay request id. The
  engine's own id is masked before grouping: one line, `count: 20`,
  response 2.9 kB instead of 5.2.
- **The JSON-RPC error path.** `mcp_client.call` assumed every response
  had a `result`; a refused call (bad template id) crashed the check
  script with a `KeyError`. It now raises the tool's own message.

---

## Reproducing this

```bash
just build && just mcp-up && just tf-setup     # engine serves get_exemplar_request + replay_exemplar; rawtools serves http_request
S1_FAST=1 just scenario s1                     # fresh fault, left active
just s8-check                                  # exemplar, replay, control, clean
DEMO_APPROVAL=allow just demo                  # Spyglass with SOP v5 (causal check before the gate)
```

---

## Spec revisions this phase forces

1. **C9's executor** is the evidence engine (ADR-010 amended): the ledger
   example's `sandbox.replay` entries become one `replay_exemplar` entry
   with two eids; "the agent writes a replay script" becomes "the agent
   designs the experiment".
2. **`get_exemplar_request`** accepts `eid` and `event_id` as well as
   `template_id` / `route + status`; its default window is all history
   (the *first* failing request); it returns the `chain` and the 5xx
   `origin`, and says whether the request is replayable.
3. **`replay_exemplar`** is the tool's name and shape: `comparison`
   {proportions, Δ, threshold, verdict, reading}; `n ≤ 50`; the experiment's
   traffic is excluded from evidence; side effects are stated on the
   result and in `docs/safety.md` (the read/write separation gains its one
   exception).
4. **SOP step 6's ACT exit** requires a separated replay when a replay was
   possible; "replay not possible" is the spec's bisection fallback, and the
   RCA says so.
5. **Step 3's fan-out** is conditional on the bundle; the briefs are prose
   the lead pastes, not static sub-agent definitions.
6. **ADR-012's information-access mapping** gains a row: `http_request`
   (raw) ↔ `get_exemplar_request` + `replay_exemplar` (shaped); Phase 10
   reports tokens next to the RCA's label (causal / correlational).
