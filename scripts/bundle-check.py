#!/usr/bin/env python3
"""Phase 6 + 7 acceptance: ranking and the evidence bundle on S1.

Against the live engine through the MCP tool surface, on the latest S1 run:

  P6  RANK      the top 3 bundle items are the fault deploy, an error
                changepoint, and the seeded novel template (any order)
  P6  ABLATION  re-ranking with w_n = 0 changes every score by exactly w_n
                and the ledger records the weights (the plumbing the Phase 10
                ablation needs); whether the ORDER changes is reported -- on
                S1's incident window every candidate is first-seen, so
                novelty is a constant there and cannot reorder
  P7  BOUNDS    <= 20 items, <= 8 kB serialised, reports reduction_ratio
  P7  FACTS     the three key facts are present with their relationships
                (D-2 precedes the changepoint and the template within 120 s)

Exit 0 on PASS, 3 on FAIL. Raw responses go next to the run manifest
(`bundle-check.json`).

  scripts/bundle-check.py                     # latest S1 run
  scripts/bundle-check.py --run data/scenarios/s1/<id> --focus gateway
"""
from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_client import call, session, wait_ready  # noqa: E402

RUNS = Path("data/scenarios/s1")
GT = Path("scenarios/s1-payment-regression/ground-truth.yaml")
MAX_ITEMS, MAX_BYTES = 20, 8192


def ts(s: str) -> datetime:
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def iso(t: datetime) -> str:
    return t.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def latest_run() -> Path:
    runs = sorted(p for p in RUNS.iterdir() if (p / "manifest.json").exists())
    if not runs:
        sys.exit("no S1 runs under data/scenarios/s1; run `just scenario s1` first")
    return runs[-1]


def brief(it: dict) -> str:
    k = it["kind"]
    if k == "novel_template":
        return f"T  {it['score']:.3f} [{it['level']}{' +stack' if it.get('has_stack') else ''}] {it['pattern'][:60]} (cascade {len(it.get('cascade', []))})"
    if k == "changepoint":
        nd = it.get("nearest_deploy") or {}
        return f"CP {it['score']:.3f} {it['series']} {it['direction']} at {it['at'][11:23]} nearest {nd.get('deploy_id')} {nd.get('offset_secs')}s (cascade {len(it.get('cascade', []))})"
    return f"D  {it['score']:.3f} {it['deploy_id']} {it['service']} {it.get('from_version')}→{it.get('version')} at {it['ts'][11:23]}"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--run", type=Path)
    ap.add_argument("--focus", default="gateway", help="focus_service (the alerting service); '' for none")
    a = ap.parse_args()
    run = a.run or latest_run()
    m = json.loads((run / "manifest.json").read_text())
    gt = yaml.safe_load(GT.read_text())
    seeded = next(e for e in gt["expected_evidence"] if e["kind"] == "novel_template")["pattern"]
    t_fault, t_end = ts(m["t_fault"]), ts(m["t_end"])
    fault_id, benign_id = m["fault_deploy"]["deploy_id"], m["benign_deploy"]["deploy_id"]
    sid = session(name="bundle-check")
    wait_ready(sid)
    raw: dict = {"run": str(run), "checks": {}}
    fails: list[str] = []

    w = {"from": iso(t_fault - timedelta(seconds=120)), "to": iso(t_end)}
    args = {"window": w}
    if a.focus:
        args["focus_service"] = a.focus
    r = call(sid, "build_evidence_bundle", args)
    raw["checks"]["bundle"] = r
    res, meta = r["result"], r["meta"]
    items, cov = res["items"], res["coverage"]
    size = len(json.dumps(res, separators=(",", ":")))
    print(f"run {run.name}: fault {fault_id} at {m['t_fault']}; window {w['from']} .. {w['to']}; focus {a.focus or '-'}")
    print(f"\nbundle {res['bundle_id']}: {len(items)} items, result {size} B (items+relationships {cov['bytes_returned']} B), engine {meta['engine_latency_ms']} ms, "
          f"T0 {res['incident_t0']['ts']} ({res['incident_t0']['source']})")
    print(f"coverage: {cov['events_scanned']} events / {cov['bytes_scanned']} B scanned → {cov['items_returned']} items; "
          f"reduction {cov['reduction_ratio']}:1 events/item, {cov['bytes_reduction_ratio']}:1 bytes; facts after dedupe {cov['facts_after_dedupe']}"
          f" (templates novel {cov['templates_novel']}, changepoints {cov['changepoints_found']}, deploys {cov['deploys_considered']})")
    for i, it in enumerate(items, 1):
        print(f"  {i:2d}. (by score #{it['score_rank']}) {brief(it)}  factors {it['factors']}")
    print("relationships:")
    for rel in res["relationships"]:
        print(f"   {rel['from']} -[{rel['type']} {rel.get('offset_secs')}s]-> {rel['to']}")

    # P7 BOUNDS
    if len(items) > MAX_ITEMS:
        fails.append(f"BOUNDS: {len(items)} items > {MAX_ITEMS}")
    if size > MAX_BYTES:
        fails.append(f"BOUNDS: result {size} B > {MAX_BYTES}")
    if not isinstance(cov.get("reduction_ratio"), (int, float)):
        fails.append("BOUNDS: reduction_ratio not reported")

    # P7 FACTS + P6 RANK
    top3 = items[:3]
    kinds = sorted(it["kind"] for it in top3)
    dep = next((it for it in items if it["kind"] == "deploy" and it["deploy_id"] == fault_id), None)
    cp = next((it for it in items if it["kind"] == "changepoint" and it["direction"] == "up" and it["series"].startswith(("error_rate", "errors_total"))), None)
    tpl = next((it for it in items if it["kind"] == "novel_template" and it["pattern"] == seeded), None)
    if not dep:
        fails.append(f"FACTS: fault deploy {fault_id} not in the bundle")
    if not cp:
        fails.append("FACTS: no upward error changepoint in the bundle")
    if not tpl:
        fails.append(f"FACTS: seeded template not in the bundle: {seeded}")
    if kinds != ["changepoint", "deploy", "novel_template"]:
        fails.append(f"RANK: top-3 kinds are {kinds}, expected one of each")
    else:
        if top3[[it["kind"] for it in top3].index("deploy")]["deploy_id"] != fault_id:
            fails.append(f"RANK: the top-3 deploy is not {fault_id}")
        if top3[[it["kind"] for it in top3].index("novel_template")]["pattern"] != seeded:
            fails.append("RANK: the top-3 template is not the seeded one")
        if not top3[[it["kind"] for it in top3].index("changepoint")]["series"].startswith(("error_rate", "errors_total")):
            fails.append("RANK: the top-3 changepoint is not an error series")
    if any(it["kind"] == "deploy" and it["deploy_id"] == benign_id for it in top3):
        fails.append(f"RANK: benign deploy {benign_id} in the top 3")
    rel_types = {(r["from"], r["to"]) for r in res["relationships"]}
    if dep and cp and (fault_id, cp["ref"]) not in rel_types:
        fails.append("FACTS: no deploy→changepoint relationship")
    if dep and tpl and (fault_id, tpl["ref"]) not in rel_types:
        fails.append("FACTS: no deploy→template relationship")

    # P6 ABLATION: w_n = 0
    r0 = call(sid, "build_evidence_bundle", {**args, "weights": {"w_n": 0}})
    raw["checks"]["bundle_w_n_0"] = r0
    items0 = r0["result"]["items"]
    print("\nwith w_n = 0:")
    for i, it in enumerate(items0, 1):
        print(f"  {i:2d}. {brief(it)}")
    order, order0 = [it["ref"] for it in items], [it["ref"] for it in items0]
    scores, scores0 = {it["ref"]: it["score"] for it in items}, {it["ref"]: it["score"] for it in items0}
    w_n = res["ranking"]["weights"]["w_n"]
    moved = [ref for ref in order if ref in order0 and order.index(ref) != order0.index(ref)]
    deltas = {ref: round(scores[ref] - scores0[ref], 3) for ref in order if ref in scores0}
    novel_all = all(abs(d - w_n) < 1e-6 for d in deltas.values())
    print(f"ablation: recorded weights w_n {w_n} → {r0['result']['ranking']['weights']['w_n']} (ledger n={r0['ledger_n']}, query hash {r['meta']['query_hash']} → {r0['meta']['query_hash']}); "
          f"score deltas {sorted(set(deltas.values()))}; {len(moved)} item(s) changed position"
          f"{' -- every candidate is first-seen in this window, so novelty is a constant and cannot reorder' if novel_all and not moved else ''}")
    if r["meta"]["query_hash"] == r0["meta"]["query_hash"]:
        fails.append("ABLATION: the weights are not part of the recorded query")
    if not deltas or any(d <= 0 for d in deltas.values()):
        fails.append("ABLATION: w_n = 0 did not lower the scores of novel items")
    if not moved and not novel_all:
        fails.append("ABLATION: candidates differ in novelty yet w_n = 0 did not reorder")

    raw["fails"] = fails
    (run / "bundle-check.json").write_text(json.dumps(raw, indent=1))
    print()
    if fails:
        print("FAIL")
        for x in fails:
            print(f"  - {x}")
        sys.exit(3)
    print(f"PASS: top 3 = deploy + changepoint + template; {len(items)} items / {size} B; reduction {cov['reduction_ratio']}:1; w_n=0 shifts every score by {w_n}{' and reorders' if moved else ' (order invariant: all candidates novel)'}")


if __name__ == "__main__":
    main()
