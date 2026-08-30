#!/usr/bin/env python3
"""Error-rate and latency curve of a scenario run, and run-vs-run comparison.

The scenario acceptance test (Phase 1's bar, applied to every scenario in
Phase 10): two runs from clean state must produce the same curve within the
tolerances pre-registered in the scenario's ground-truth.yaml. Per 10 s bucket
relative to the fault timestamp in the run manifest: request count, 5xx share
of /checkout (4xx excluded by construction) and p95 latency.

  scripts/scenario-curve.py --scenario s2               # the latest s2 run
  scripts/scenario-curve.py --scenario s2 --compare     # the two latest runs
  scripts/scenario-curve.py --scenario s6 --compare data/scenarios/s6/<a> data/scenarios/s6/<b>
"""
import argparse
import glob
import json
import statistics
import sys
from datetime import datetime
from pathlib import Path

import yaml

BUCKET = 10


def ts(s: str) -> datetime:
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def gt_path(scenario: str) -> Path:
    hits = sorted(glob.glob(f"scenarios/{scenario}-*/ground-truth.yaml"))
    if not hits:
        sys.exit(f"no scenarios/{scenario}-*/ground-truth.yaml")
    return Path(hits[0])


def curve(run: Path):
    m = json.loads((run / "manifest.json").read_text())
    t_fault = ts(m["t_fault"])
    buckets: dict[int, dict] = {}
    for line in (run / "logs" / "gateway.jsonl").read_text().splitlines():
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        if r.get("route") != "/checkout" or "status" not in r:
            continue
        b = int((ts(r["ts"]) - t_fault).total_seconds() // BUCKET) * BUCKET
        d = buckets.setdefault(b, {"n": 0, "e": 0, "lat": []})
        d["n"] += 1
        d["e"] += r["status"] >= 500
        d["lat"].append(float(r.get("latency_ms", 0)))
    pts = []
    for b, d in sorted(buckets.items()):
        lat = sorted(d["lat"])
        p95 = lat[min(len(lat) - 1, int(len(lat) * 0.95))] if lat else 0.0
        pts.append((b, d["n"], d["e"] / d["n"] if d["n"] else 0.0, p95))
    return m, pts


def evaluate(m, pts, gt):
    er = gt["expected_error_rate"]
    lt = gt.get("expected_latency")
    warm = -(m["steady_secs"] + m.get("lead_secs", 0)) + 20          # skip startup warm-up
    pre = [(r, p) for b, n, r, p in pts if warm <= b < -BUCKET and n >= 5]
    post = [(r, p) for b, n, r, p in pts if 20 <= b < m["post_secs"] - BUCKET and n >= 5]
    ok_pre = bool(pre) and max(r for r, _ in pre) <= er["pre_fault_max"]
    mean_post = statistics.mean(r for r, _ in post) if post else 0.0
    ok_post = bool(post) and er["post_fault"]["min"] <= mean_post <= er["post_fault"]["max"]
    ev = {"pre_max": max((r for r, _ in pre), default=None), "pre_windows": len(pre),
          "post_mean": mean_post, "post_min": min((r for r, _ in post), default=None),
          "post_max": max((r for r, _ in post), default=None), "post_windows": len(post),
          "ok_error_rate": ok_pre and ok_post, "ok": ok_pre and ok_post}
    if lt:
        pre_p95 = max((p for _, p in pre), default=0.0)
        post_p95 = statistics.median(p for _, p in post) if post else 0.0
        ok_lat = pre_p95 <= lt["pre_fault_p95_max_ms"] and post_p95 >= lt["post_fault_p95_min_ms"]
        ev.update({"pre_p95_max_ms": pre_p95, "post_p95_median_ms": post_p95, "ok_latency": ok_lat})
        ev["ok"] = ev["ok"] and ok_lat
    return ev


def show(run: Path, m, pts, ev):
    fault = m.get("fault_deploy", {}).get("deploy_id") or m.get("fault", {}).get("kind", "?")
    print(f"\n== {run.name}  ({m['scenario']}, fast={m.get('fast')}, seed={m['seed']}, rate={m['rate']}, fault={fault} at {m['t_fault']})")
    print(f"   {'t-fault':>8}  {'reqs':>5}  {'5xx%':>6}  {'p95 ms':>8}")
    for b, n, r, p in pts:
        bar = "#" * int(r * 50)
        mark = " <- fault" if b == 0 else ""
        print(f"   {b:>+8}s  {n:>5}  {100*r:>5.1f}  {p:>8.0f}  {bar}{mark}")
    print(f"   pre-fault 5xx max {100*(ev['pre_max'] or 0):.1f}% over {ev['pre_windows']} windows | "
          f"post-fault 5xx mean {100*ev['post_mean']:.1f}% "
          f"[{100*(ev['post_min'] or 0):.1f}..{100*(ev['post_max'] or 0):.1f}] over {ev['post_windows']} windows "
          f"-> {'ok' if ev['ok_error_rate'] else 'FAIL'}")
    if "ok_latency" in ev:
        print(f"   pre-fault p95 max {ev['pre_p95_max_ms']:.0f} ms | post-fault p95 median {ev['post_p95_median_ms']:.0f} ms "
              f"-> {'ok' if ev['ok_latency'] else 'FAIL'}")
    print(f"   -> {'PASS' if ev['ok'] else 'FAIL'}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scenario", default="s1")
    ap.add_argument("--compare", nargs="*", metavar="RUN_DIR", help="two run dirs (default: the two most recent)")
    ap.add_argument("run", nargs="?", help="single run dir to print")
    a = ap.parse_args()
    gt = yaml.safe_load(gt_path(a.scenario).read_text())
    runs_dir = Path(f"data/scenarios/{a.scenario}")

    if a.compare is not None:
        runs = [Path(p) for p in a.compare] if a.compare else sorted(runs_dir.iterdir())[-2:]
        if len(runs) != 2:
            sys.exit(f"need two runs, have {len(runs)}")
        evs = []
        for r in runs:
            m, pts = curve(r)
            ev = evaluate(m, pts, gt)
            show(r, m, pts, ev)
            evs.append(ev)
        drift = abs(evs[0]["post_mean"] - evs[1]["post_mean"])
        same = drift <= 0.05
        ok = all(e["ok"] for e in evs) and same
        print(f"\nrun-to-run post-fault 5xx drift {100*drift:.1f} pts (tolerance 5.0) -> {'same' if same else 'DIFFERENT'}")
        print(f"\n{a.scenario.upper()} REPRODUCIBILITY: {'PASS' if ok else 'FAIL'}")
        sys.exit(0 if ok else 1)

    run = Path(a.run) if a.run else sorted(runs_dir.iterdir())[-1]
    m, pts = curve(run)
    ev = evaluate(m, pts, gt)
    show(run, m, pts, ev)
    sys.exit(0 if ev["ok"] else 1)


if __name__ == "__main__":
    main()
