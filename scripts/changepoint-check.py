#!/usr/bin/env python3
"""Phase 5 acceptance: does `detect_changepoints` localise the S1 incident?

Three checks, against the live engine through the MCP tool surface, judged
by the tolerances pre-registered in ground-truth.yaml and the spec:

  1. FAULT     a changepoint on the orders /orders error series, direction
               up, |at - t_fault| <= tolerance, magnitude >= the floor, and
               the nearest deploy is the fault deploy (D-2), 0..60 s before it
  2. DECOY     no changepoint in the pre-fault window, which contains the
               benign deploy D-1 (a real deploy that changed nothing)
  3. STEADY    no changepoint on >= 10 minutes of deploy-free traffic

Exit 0 on PASS, 3 on FAIL. Raw tool responses are written next to the run
manifest (`changepoint-check.json`) so the numbers in the findings are
traceable.

  scripts/changepoint-check.py                    # latest S1 run under data/scenarios/s1
  scripts/changepoint-check.py --run data/scenarios/s1/<id>
  scripts/changepoint-check.py --steady 2026-08-29T04:13:30Z,2026-08-29T04:23:30Z
"""
from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_client import call, session  # noqa: E402

RUNS = Path("data/scenarios/s1")
GT = Path("scenarios/s1-payment-regression/ground-truth.yaml")
SPEC_TOLERANCE_SECS = 10      # README Phase 5 acceptance: "within ±10s of injected truth"
STEADY_MIN_SECS = 600         # "no changepoints on 10 minutes of steady state"


def ts(s: str) -> datetime:
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def iso(t: datetime) -> str:
    return t.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def latest_run() -> Path:
    runs = sorted(p for p in RUNS.iterdir() if (p / "manifest.json").exists())
    if not runs:
        sys.exit("no S1 runs under data/scenarios/s1; run `just scenario s1` first")
    return runs[-1]


def series_matches(item: dict, labels: dict) -> bool:
    return all(item.get("labels", {}).get(k) == v for k, v in labels.items())


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--run", type=Path, help="run dir with manifest.json (default: latest)")
    ap.add_argument("--steady", help="explicit steady-state window `from,to` (RFC3339); default: longest deploy-free span")
    ap.add_argument("--tolerance", type=float, default=SPEC_TOLERANCE_SECS)
    a = ap.parse_args()

    run = a.run or latest_run()
    m = json.loads((run / "manifest.json").read_text())
    gt = yaml.safe_load(GT.read_text())
    gt_cp = next(e for e in gt["expected_evidence"] if e["kind"] == "changepoint")
    gt_corr = next(e for e in gt["expected_evidence"] if e["kind"] == "deploy_correlation")
    t_fault, t_benign, t_start, t_end = ts(m["t_fault"]), ts(m["t_benign_deploy"]), ts(m["t_start"]), ts(m["t_end"])
    fault_id = m["fault_deploy"]["deploy_id"]
    benign_id = m["benign_deploy"]["deploy_id"]
    labels = {"service": "orders", "route": "/orders"}   # ground truth: errors_total{service="orders",route="/orders"}

    sid = session(name="changepoint-check")
    raw: dict = {"run": str(run), "checks": {}}
    fails: list[str] = []
    print(f"run {run.name}: fault {fault_id} at {m['t_fault']}, benign {benign_id} at {m['t_benign_deploy']}")

    # 1. FAULT ---------------------------------------------------------------
    w = {"from": iso(t_fault - timedelta(seconds=60)), "to": iso(t_end)}
    r = call(sid, "detect_changepoints", {"window": w})
    raw["checks"]["fault"] = r
    items = r["result"]["items"]
    print(f"\n[1] FAULT window {w['from']} .. {w['to']}: {len(items)} changepoint(s) on {r['result']['series_changed']} of {r['result']['series_scanned']} series, "
          f"engine {r['meta']['engine_latency_ms']} ms")
    for it in items[:8]:
        print(f"    {it['headline']}")
    hits = [it for it in items if series_matches(it, labels) and it["metric"] in ("errors_total", "error_rate") and it["direction"] == "up"]
    if not hits:
        fails.append("FAULT: no upward changepoint on orders /orders error series")
    else:
        best = min(hits, key=lambda it: abs((ts(it["at"]) - t_fault).total_seconds()))
        off = (ts(best["at"]) - t_fault).total_seconds()
        mag = best["magnitude_x"]
        mag_ok = mag == "new" or (isinstance(mag, (int, float)) and mag >= gt_cp["magnitude_min_x"])
        nd = best.get("nearest_deploy") or {}
        dep_ok = nd.get("deploy_id") == fault_id and 0 <= nd.get("offset_secs", -1) <= gt_corr["changepoint_after_deploy_secs_max"]
        print(f"    -> {best['series']}: at {best['at']} ({off:+.1f} s vs truth; spec ±{a.tolerance:.0f}, ground truth ±{gt_cp['tolerance_secs']}), "
              f"magnitude {mag} (>= {gt_cp['magnitude_min_x']}), z {best['z']}, nearest deploy {nd.get('deploy_id')} {nd.get('offset_secs')} s")
        if abs(off) > a.tolerance:
            fails.append(f"FAULT: changepoint {off:+.1f} s from truth exceeds ±{a.tolerance} s")
        if not mag_ok:
            fails.append(f"FAULT: magnitude {mag} below {gt_cp['magnitude_min_x']}x")
        if not dep_ok:
            fails.append(f"FAULT: nearest deploy is {nd.get('deploy_id')} at {nd.get('offset_secs')} s, expected {fault_id} within 0..{gt_corr['changepoint_after_deploy_secs_max']} s")
        rank = items.index(best) + 1
        print(f"    -> rank {rank} of {len(items)}; first item: {items[0]['series']} at {items[0]['at']} nearest {((items[0].get('nearest_deploy') or {}).get('deploy_id'))}")
        blamed_benign = [it for it in items if (it.get("nearest_deploy") or {}).get("deploy_id") == benign_id]
        if blamed_benign:
            fails.append(f"FAULT: {len(blamed_benign)} changepoint(s) annotated with the benign deploy {benign_id}")

    # 2. DECOY ---------------------------------------------------------------
    w = {"from": iso(t_start + timedelta(seconds=60)), "to": iso(t_fault - timedelta(seconds=1))}
    r = call(sid, "detect_changepoints", {"window": w})
    raw["checks"]["decoy"] = r
    items = r["result"]["items"]
    span = (t_fault - t_start).total_seconds() - 61
    print(f"\n[2] DECOY window (pre-fault, contains {benign_id} at {m['t_benign_deploy']}), {span:.0f} s: {len(items)} changepoint(s) on {r['result']['series_scanned']} series"
          f"{'' if not r['result']['unconfirmed_tail'] else f'; unconfirmed tail: {r['result']['unconfirmed_tail']}'}")
    for it in items[:5]:
        print(f"    {it['headline']}")
    if items:
        fails.append(f"DECOY: {len(items)} changepoint(s) in the pre-fault window")

    # 3. STEADY --------------------------------------------------------------
    if a.steady:
        f, t = a.steady.split(",")
        steady = (ts(f), ts(t))
        how = "explicit"
    else:
        journal = [json.loads(l) for l in Path("data/deploy/journal.jsonl").read_text().splitlines() if l.strip()]
        wm = ts(call(sid, "freshness_watermark", {})["result"]["newest_log_ts"])
        last_change = max(ts(j["ts"]) for j in journal if j.get("deploy_id")) if journal else t_end
        candidates = [((last_change + timedelta(seconds=30)), wm, "after the last deploy/rollback"),
                      (t_start + timedelta(seconds=60), t_fault - timedelta(seconds=1), "pre-fault")]
        steady, how = None, None
        for f, t, label in candidates:
            if (t - f).total_seconds() >= STEADY_MIN_SECS:
                steady, how = (f, t), label
                break
    if steady is None:
        print(f"\n[3] STEADY: no deploy-free span of >= {STEADY_MIN_SECS} s available yet (wait, or pass --steady)")
        fails.append("STEADY: not evaluated")
    else:
        w = {"from": iso(steady[0]), "to": iso(steady[1])}
        r = call(sid, "detect_changepoints", {"window": w})
        raw["checks"]["steady"] = r
        items = r["result"]["items"]
        span = (steady[1] - steady[0]).total_seconds()
        print(f"\n[3] STEADY window ({how}) {w['from']} .. {w['to']}, {span/60:.1f} min: {len(items)} changepoint(s) on {r['result']['series_scanned']} series, "
              f"{r['result']['buckets_evaluated']} buckets"
              f"{'' if not r['result']['unconfirmed_tail'] else f'; unconfirmed tail: {r['result']['unconfirmed_tail']}'}")
        for it in items[:5]:
            print(f"    {it['headline']}")
        if span < STEADY_MIN_SECS:
            fails.append(f"STEADY: window is {span:.0f} s, need {STEADY_MIN_SECS}")
        if items:
            fails.append(f"STEADY: {len(items)} changepoint(s) on steady traffic")

    raw["fails"] = fails
    (run / "changepoint-check.json").write_text(json.dumps(raw, indent=1))
    print()
    if fails:
        print("FAIL")
        for f in fails:
            print(f"  - {f}")
        sys.exit(3)
    print("PASS: fault localised, decoy deploy not blamed, steady state clean")


if __name__ == "__main__":
    main()
