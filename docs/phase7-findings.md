# Phase 7 — Evidence bundles: build record

**Objective (spec):** `build_evidence_bundle` per C6 with bounds and coverage
stats. The "184,203 events → 12 items" line.
**Built:** 2026-08-29, 13:50–15:00 IST (08:20–09:30 UTC; run files 09:20–09:29 UTC), together with
Phase 6 · **PR:** #7
**Acceptance bar (spec):** bundle for S1 ≤ 20 items / ≤ 8 KB total, contains
the three key facts, reports `reduction_ratio`; SOP starts from the bundle.

---

## Status summary

| Spec task | Status | Where |
|---|---|---|
| `build_evidence_bundle(window, focus_service?, limit?, weights?)` | ✅ candidates from the tools' own functions over one frozen window; deduped, scored, kind-diverse head, byte-bounded; deterministic, re-checks from the ledger | `crates/spyglass-engine/src/bundle.rs` |
| Bounds, engine-enforced | ✅ ≤ `bounds.max_items` (20), ≤ `bounds.max_bytes_per_item` (2 kB), ≤ `bundle.max_bytes` (8 kB) for the whole payload — items are added in rank order until the budget is spent, and the payload is re-checked and trimmed from the tail | `spyglass.toml [bundle]` |
| `coverage` | ✅ events and bytes scanned, templates in window / novel, changepoints found, deploys considered, facts after dedupe, items returned, truncated, bytes returned, `reduction_ratio` (events per item) and `bytes_reduction_ratio` | |
| `relationships` | ✅ deploy → change events within 120 s (`precedes_within_120s`, offset); changepoint ↔ template within 2 s (`coincides_within_2s`); by stable refs, not eids, so the digest is session-independent (F2) | |
| Compact items, full records | ✅ the bundle carries pointers and numbers; `get_evidence(eid)` returns the full record with the raw excerpt (F3) | `spyglass-mcp::respond` |
| **Acceptance: ≤ 20 items / ≤ 8 kB, three key facts, `reduction_ratio`** | ✅ 6 items, 5,562 B, **8,747 events → 6 items (1458 : 1), 2.74 MB → 4.3 kB (630 : 1)**; `D-2`, the error changepoint and the seeded template present with their relationships (F1) | `just s7-check` |
| **SOP starts from the bundle** | ✅ SOP v4: `freshness_watermark → build_evidence_bundle(focus_service)`, two triage calls; budget "under 6 tool calls" | `agent/sop.md` |

---

## Findings and decisions

### F1. Acceptance, measured

Fast-timeline run `20260829T080250Z`, window `[D-2 − 120 s, end]` (190 s),
focus `gateway`:

```
bundle B-ec7362b82b4d: 6 items, 5,562 B, engine 61 ms, T0 08:04:21.608 (earliest_error_changepoint)
coverage: 8,747 events / 2,741,253 B scanned → 6 items; 1458:1 events per item, 630:1 bytes
          templates in window 21, novel 6 → facts after dedupe 6; changepoints found 4; deploys 2
relationships:
  D-2 -[precedes_within_120s +0.6 s]-> T21 (the seeded template)
  D-2 -[precedes_within_120s +0.6 s]-> error_rate{payments,/charge}
  D-2 -[precedes_within_120s +0.1 s]-> T20 (the INFO decoy)
  D-1 -[precedes_within_120s +50.7 s]-> T21, the changepoint, T20     (true on the 90 s fast timeline)
  error_rate{payments,/charge} -[coincides_within_2s 0.0 s]-> T21
```

Items and their scores are in `phase6-findings.md` F1. Five of the six
items are what an investigator needs; the sixth (`loadgen window sent=…`)
is a fast-timeline artefact (Phase 6 F5). The reduction ratio the spec
wanted on screen is **8,747 → 6**; on the default timeline's larger window
it is larger still, and the number is computed per call, never typed.

### F2. Relationships reference refs, not eids

The spec's example links items by evidence id. Evidence ids are issued per
investigation (E1, E2 … per MCP session), and the ledger digest strips
them so identical evidence yields identical digests across sessions
(ADR-004). A relationship `{"from": "E1", "to": "E2"}` would put the
session's numbering back into the digest and break the re-check. Items
therefore carry a stable `ref` (`template_id`, the series key, the
`deploy_id`) and relationships use those; every item also carries its
eid, so a claim still cites an eid.

### F3. The bundle is pointers and numbers; the excerpt is one dereference away

A template item in the bundle is ~450 bytes: pattern, level, stack flag,
novelty and reason, first seen, counts, services, the cascade, the
exemplar event ids and `excerpt_bytes`. The 600-byte raw excerpt is not
in it. The MCP layer now takes a parallel list of *full records* from the
tool and stores those as the evidence the eids dereference to, so
`get_evidence(E1)` returns the excerpt and the exemplars exactly as
`novel_templates` would have. Six templates with excerpts would have been
the whole byte budget; the SOP's one `get_evidence` on the top ERROR item
buys the stack trace for the one item that needs it.

### F4. Bytes are the budget, not a cap

Items are added in rank order while the running size stays under
`max_bytes − 1400` (the envelope and relationships reserve); the assembled
payload is then measured and, if over, trimmed from the tail with the
relationships recomputed. `coverage.truncated` and `facts_after_dedupe`
say what was left out. On S1 nothing is: six facts, 5.6 kB. The bound was
never hit — which is the dedupe doing its job, not the bound being loose.

### F5. The agent runs

Two `just demo` runs with SOP v4 (bundle-first), the same fault, the same
approval policy. Run 1 found a determinism bug (F6); run 2 is the record.

| Metric | P7 run 1 | P7 run 2 | P5 (four-call triage) | P4 (novelty) | Baseline |
|---|---|---|---|---|---|
| Outcome | completed: correct RCA, `D-3` citing E1/E2/E3, **20.6 % → 0.0 %** | completed: correct RCA, `D-3` citing E1/E2/E3, **20.6 % → 0.0 %** | completed | completed | completed |
| Tool calls | **11** (`build_evidence_bundle` 1, `get_evidence` 1, `current_versions` 1, `rollback` 1, verification 7) | **11** (same sequence) | 13 | 12 | 19 |
| Model calls | 10 | 12 | 11 | 9 | 11 |
| Input tokens | **177,672** | **184,309** | 222,647 | 141,601 | 198,106 |
| Output tokens | 7,527 | 6,421 | 8,186 | 7,551 | 4,978 |
| Peak context | 23.4k | 21.2k | 26.4k | 21.5k | 30.0k |
| Tool bytes → context | 30,288 (bundle **6,765**; `get_evidence` 11,951) | **26,536** (bundle 6,764; `get_evidence` 7,999) | 38,133 | 31,363 | 57,147 |
| Wall | 84.6 s | 71.5 s | 92.7 s | 70.5 s | 39.5 s |
| Evidence ids cited | **11 of 11** | **11 of 11** | 16 of 18 | 14 of 14 | none exist |
| Ledger re-check | FAIL 2/3 (F6) | **PASS 3/3** | PASS 5/5 | PASS 4/4 | n/a |

The triage is two calls. The bundle (6.8 kB) replaced `novel_templates` +
`detect_changepoints` + `deploy_events` (7.9 + 6.3 + 1.5 kB); input tokens
fell to **177.7k and 184.3k** — 17–20 % below Phase 5 and **7–10 % below
the baseline** on both runs — the first Spyglass runs with changepoints in
the evidence to beat it. Run 2 spent two more model calls than run 1 on the
same tool sequence (model nondeterminism; n = 1 per row), with less
context per call (peak 21.2k) and **re-check PASS 3/3** on the safe
watermark. Still 25 % above Phase 4's 141.6k: the
bundle carries the changepoint and deploy facts Phase 4's run never
fetched, and the postmortem uses them (the cascade, the +0.6 s offset, the
50 s silence after `D-1`). The largest response in the run was no longer
the bundle but `get_evidence` (11.9 kB: three full stack traces for one
template) — fixed for run 2 by returning the first exemplar whole and
capping the rest to an excerpt.

Run 1's postmortem: *"Error cascade propagated downstream from `payments`
to `orders` (HTTP 500 charge failure) and `gateway` (`/checkout` 5xx rate
elevated to ~18.7 %) [E1, E2]"*; *"The deploy-then-error relationship is
CORRELATIONAL (error onset occurred +0.625 s after deploy timestamp `D-2`)
[E2, E3]"*; `D-1` rejected: *"50.6 seconds prior to error onset. Telemetry
showed no errors during this 50 s window"*.

### F6. One late event broke the digest — and fixed the watermark

Run 1's re-check failed on the bundle: recorded `9,561 events / 3,032,044 B`
in the window, replayed `9,562 / 3,032,271`. One event, 227 bytes. The
window's end was the *newest* ingested timestamp across all files; the
tailer reads the files one after another, so a line written to an
already-read file a few milliseconds before another file's newest line is
inside the window and not yet in the store. It arrives on the next poll and
the replay sees it. Phase 3's one mismatch (its F6) was very likely the
same thing.

Every window now resolves its end at the **safe watermark**: the newest
timestamp every *active* source has been read past (a source more than 5 s
behind the newest is idle — payments-v2 after a rollback — and does not
hold it back). Per-file timestamps are monotonic, so a line with
`ts ≤ safe` has been ingested from every file; a requested end past it is
clamped and the clamped window is what the ledger records.
`freshness_watermark` reports `safe_log_ts` and `safe_lag_ms` next to the
newest. ADR-004 gains the rule.

---

## Reproducing this

```bash
just build && just mcp-up && just tf-setup     # engine now serves build_evidence_bundle
just s7-check                                  # bounds, facts, relationships, ranking
DEMO_APPROVAL=allow just demo                  # Spyglass with SOP v4 (bundle-first)
```

---

## Spec revisions this phase forces

1. **C6's bundle** carries `ref` on every item and links relationships by
   ref (F2); `coverage` gains `bytes_scanned`, `bytes_reduction_ratio`,
   `facts_after_dedupe`, `truncated`; the bundle gains `incident_t0` and
   `ranking` (weights used, order rule).
2. **Items are compact by design** (F3); "one exemplar excerpt per
   template" becomes "one exemplar excerpt per `get_evidence`".
3. **The 8 kB acceptance** is `bundle.max_bytes` in config, enforced on
   the whole payload (F4).
4. **Windows end at the safe watermark** (F6): the README's
   `freshness_watermark` row and ADR-004 say so; `get_evidence` returns
   one whole exemplar and capped copies.
