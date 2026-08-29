#!/usr/bin/env python3
"""Phase 8 acceptance: the causal check on S1, measured.

Against the live engine through the MCP tool surface, on the latest S1 run
(the fault must still be active -- run this before the agent rolls it back,
or right after `just scenario s1`):

  EXEMPLAR  get_exemplar_request on the seeded template returns one captured
            non-USD checkout, sanitized (no auth-shaped header survives), with
            its chain through the services and payments-v2 as the 5xx origin
  REPLAY    replay_exemplar v1 vs v2, N=20: v1 <= 1/20, v2 >= 19/20 (the
            spec's "~0/20 vs ~19-20/20, measured, not asserted"),
            verdict `separated`, two evidence ids, one ledger entry
  CONTROL   a request that succeeded, replayed the same way, fails on
            neither version: `not_separated` -- the tool can say no
  CLEAN     the experiment is not evidence of itself: every replay-tagged
            line the raw logs gained was excluded by the engine (counted
            both ways), and payments-v1 -- which has no live traffic while
            the fault routes to v2 -- shows zero requests in the replay window

Exit 0 on PASS, 3 on FAIL. Raw responses go next to the run manifest
(`replay-check.json`).

  scripts/replay-check.py                    # latest S1 run
  scripts/replay-check.py --run data/scenarios/s1/<id> --n 20
"""
from __future__ import annotations

import argparse
import json
import re
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_client import call, session, wait_ready  # noqa: E402

RUNS = Path("data/scenarios/s1")
GT = Path("scenarios/s1-payment-regression/ground-truth.yaml")
AUTH_SHAPED = re.compile(r"auth|token|secret|session|cookie|credential|password|signature|api-key", re.I)


def ts(s: str) -> datetime:
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def iso(t: datetime) -> str:
    return t.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def latest_run() -> Path:
    runs = sorted(p for p in RUNS.iterdir() if (p / "manifest.json").exists())
    if not runs:
        sys.exit("no S1 runs under data/scenarios/s1; run `just scenario s1` first")
    return runs[-1]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--run", type=Path)
    ap.add_argument("--n", type=int, default=20)
    a = ap.parse_args()
    run = a.run or latest_run()
    m = json.loads((run / "manifest.json").read_text())
    gt = yaml.safe_load(GT.read_text())
    seeded = next(e for e in gt["expected_evidence"] if e["kind"] == "novel_template")["pattern"]
    culprit = gt["culprit"]["change"]
    t_fault, t_end = ts(m["t_fault"]), ts(m["t_end"])
    sid = session(name="replay-check")
    wait_ready(sid)
    raw: dict = {"run": str(run), "checks": {}}
    fails: list[str] = []
    print(f"run {run.name}: fault {m['fault_deploy']['deploy_id']} ({culprit['from_version']}→{culprit['version']}) at {m['t_fault']}")

    # The seeded template, via the bundle over the incident window.
    w = {"from": iso(t_fault - timedelta(seconds=120)), "to": iso(t_end)}
    b = call(sid, "build_evidence_bundle", {"window": w, "focus_service": "gateway"})
    raw["checks"]["bundle"] = b
    tpl = next((it for it in b["result"]["items"] if it["kind"] == "novel_template" and it["pattern"] == seeded), None)
    if not tpl:
        sys.exit(f"FAIL: seeded template not in the bundle: {seeded}")

    # ---- EXEMPLAR ----------------------------------------------------------
    r = call(sid, "get_exemplar_request", {"eid": tpl["eid"]})
    raw["checks"]["exemplar"] = r
    ex = r["result"]["items"][0]
    req = ex["request"]
    body = json.loads(req["body"]) if req["body"].startswith("{") else {}
    origin = (ex.get("outcome") or {}).get("origin_5xx") or {}
    chain = [(c["instance"], c["status"]) for c in ex["chain"]]
    print(f"\nexemplar {ex['eid']} req {ex['req_id'][:8]} captured {ex['captured_at'][11:23]} by {ex['captured_by']} "
          f"({ex['selection'][:40]}…), engine {r['meta']['engine_latency_ms']} ms")
    print(f"  {req['method']} {req['path']} headers {sorted(req['headers'])} body {req['body']}")
    print(f"  sanitization: dropped {ex['sanitization']['headers_dropped']} redacted {ex['sanitization']['body_redactions']}")
    print(f"  chain {chain}; origin 5xx {origin.get('instance')} {origin.get('status')} stack={origin.get('has_stack')}; edge {ex['outcome'].get('edge', {}).get('status')}")
    if body.get("currency") in (None, "USD"):
        fails.append(f"EXEMPLAR: body currency is {body.get('currency')!r}, expected a non-USD failing request")
    if any(AUTH_SHAPED.search(h) for h in req["headers"]):
        fails.append(f"EXEMPLAR: an auth-shaped header survived sanitization: {sorted(req['headers'])}")
    if origin.get("instance") != "payments-v2" or not origin.get("has_stack"):
        fails.append(f"EXEMPLAR: 5xx origin is {origin}, expected payments-v2 with a stack")
    if not any(i == "gateway" and s == 502 for i, s in chain):
        fails.append(f"EXEMPLAR: chain has no gateway 502: {chain}")
    if not (t_fault <= ts(ex["captured_at"]) <= t_end):
        fails.append(f"EXEMPLAR: captured_at {ex['captured_at']} outside the fault window")
    if not ex["replay"].get("replayable"):
        fails.append(f"EXEMPLAR: not replayable: {ex['replay']}")
    if not r["meta"]["deterministic"]:
        fails.append("EXEMPLAR: get_exemplar_request must be deterministic")

    # ---- REPLAY ------------------------------------------------------------
    before = call(sid, "freshness_watermark", {})["result"]
    versions = [culprit["from_version"], culprit["version"]]
    t0 = time.time()
    rr = call(sid, "replay_exemplar", {"exemplar": ex["eid"], "service": culprit["service"] if "service" in culprit else gt["culprit"]["service"],
                                       "versions": versions, "n": a.n})
    wall = time.time() - t0
    raw["checks"]["replay"] = rr
    res, cmp_ = rr["result"], rr["result"]["comparison"]
    by_v = {it["version"]: it for it in res["items"]}
    print(f"\nreplay {res['experiment_id']} exemplar {res['exemplar']['req_id'][:8]} ({res['exemplar']['template_id']}) → {res['service']} {res['path']} n={res['n_per_version']}; "
          f"wall {wall:.2f}s, engine {rr['meta']['engine_latency_ms']} ms, eids {rr['meta']['eids']}, ledger n={rr['ledger_n']}, deterministic={rr['meta']['deterministic']}")
    for v in versions:
        it = by_v[v]
        print(f"  {it['eid']} {v} ({it['instance']}): {it['failures']}/{it['n']} failed  statuses {it['statuses']}  p50 {it['latency_ms']['p50']} ms  "
              f"{'; '.join(f'{d['status']}×{d['count']} {d['body'][:70]}' for d in it['distinct_failures']) or 'no failures'}")
    print(f"  comparison: {cmp_['proportions']} Δ {cmp_['delta']} (threshold {cmp_['min_delta_for_separation']}) → {cmp_['verdict'].upper()}")
    print(f"  reading: {cmp_['reading']}")
    good, bad = by_v[versions[0]], by_v[versions[1]]
    if good["failures"] > 1:
        fails.append(f"REPLAY: known-good {versions[0]} failed {good['failures']}/{good['n']} (expected ~0/20)")
    if bad["failures"] < bad["n"] - 1:
        fails.append(f"REPLAY: suspected {versions[1]} failed {bad['failures']}/{bad['n']} (expected ~19-20/20)")
    if cmp_["verdict"] != "separated":
        fails.append(f"REPLAY: verdict {cmp_['verdict']}, expected separated")
    if len(rr["meta"]["eids"]) != 2:
        fails.append(f"REPLAY: {len(rr['meta']['eids'])} evidence ids issued, expected 2 (one per version)")
    if rr["meta"]["deterministic"]:
        fails.append("REPLAY: a live experiment must not be marked deterministic")
    # the ledger entry
    lp = Path("ledger") / f"{rr['meta']['investigation']}.jsonl"
    entries = [json.loads(l) for l in lp.read_text().splitlines() if l.strip()] if lp.exists() else []
    entry = next((e for e in entries if e["n"] == rr["ledger_n"]), None)
    if not entry or entry["tool"] != "replay_exemplar" or sorted(entry["eids"]) != sorted(rr["meta"]["eids"]):
        fails.append(f"REPLAY: ledger entry n={rr['ledger_n']} missing or wrong: {entry and (entry['tool'], entry['eids'])}")
    else:
        print(f"  ledger {lp}: n={entry['n']} {entry['tool']} eids {entry['eids']} args {{req_id {entry['args']['req_id'][:8]}, versions {entry['args']['versions']}, n {entry['args']['n']}, body_sha256 {entry['args']['request']['body_sha256'][:12]}…}}")
        print(f"          summary: {entry['summary']}")

    # ---- CONTROL -----------------------------------------------------------
    ok = call(sid, "get_exemplar_request", {"route": req["path"], "status": 200, "window": {"from": iso(t_fault), "to": iso(t_end)}})
    raw["checks"]["control_exemplar"] = ok
    okx = ok["result"]["items"][0]
    rc = call(sid, "replay_exemplar", {"exemplar": okx["eid"], "service": gt["culprit"]["service"], "versions": versions, "n": a.n})
    raw["checks"]["control_replay"] = rc
    c = rc["result"]["comparison"]
    print(f"\ncontrol: a request that succeeded ({okx['eid']}, body {okx['request']['body']}) replayed the same way → {c['proportions']} → {c['verdict'].upper()}")
    print(f"  reading: {c['reading'][:110]}…")
    if c["verdict"] != "not_separated" or any(it["failures"] for it in rc["result"]["items"]):
        fails.append(f"CONTROL: expected 0 failures on both versions and not_separated, got {c['proportions']} {c['verdict']}")

    # ---- CLEAN -------------------------------------------------------------
    time.sleep(3.0)  # let the tailer see the replay lines
    after = call(sid, "freshness_watermark", {})["result"]
    sent = 2 * a.n * len(versions)
    excluded = after["replay_lines_excluded"] - before["replay_lines_excluded"]
    # One request can log more than one line (v2 logs its INFO decoy on a
    # successful charge), so the exact expectation is the number of lines
    # the two experiments actually produced in the raw log files.
    exps = (res["experiment_id"], rc["result"]["experiment_id"])
    in_files = sum(1 for f in Path("data/logs").glob("*.jsonl") for l in f.read_text().splitlines()
                   if any(f'"req_id":"replay-{x}-' in l for x in exps))
    d = call(sid, "error_delta", {"window_a": {"from": iso(t_fault - timedelta(seconds=60)), "to": iso(t_fault)},
                                  "window_b": {"from": res["started"], "to": iso(ts(rc["result"]["finished"]) + timedelta(seconds=2))},
                                  "group_by": "instance", "services": ["payments"]})
    raw["checks"]["clean"] = {"before": before, "after": after, "delta": d}
    v1_b = next((it["window_b"] for it in d["result"]["items"] if it["group"] == "payments-v1"), {"requests": 0})
    print(f"\nclean: {sent} requests sent → {in_files} replay-tagged lines in the raw logs; engine excluded {excluded} (ingested {before['ingested_events']} → {after['ingested_events']}); "
          f"payments-v1 requests in the replay window per error_delta: {v1_b['requests']} (live traffic routes to v2, so any request here would be leakage)")
    if excluded != in_files or in_files < sent:
        fails.append(f"CLEAN: excluded {excluded} replay lines, but the raw logs hold {in_files} (from {sent} requests)")
    if v1_b["requests"] != 0:
        fails.append(f"CLEAN: {v1_b['requests']} payments-v1 request(s) leaked into the evidence during the replay")

    raw["fails"] = fails
    (run / "replay-check.json").write_text(json.dumps(raw, indent=1))
    print()
    if fails:
        print("FAIL")
        for x in fails:
            print(f"  - {x}")
        sys.exit(3)
    print(f"PASS: exemplar sanitized; replay {versions[0]} {good['failures']}/{good['n']} vs {versions[1]} {bad['failures']}/{bad['n']} → separated; "
          f"control not_separated; ledger n={rr['ledger_n']} eids {rr['meta']['eids']}; {excluded}/{in_files} replay lines excluded, no leakage")


if __name__ == "__main__":
    main()
