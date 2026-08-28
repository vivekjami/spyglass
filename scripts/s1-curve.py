#!/usr/bin/env python3
"""S1 error-rate curve from a run's gateway log, and run-vs-run comparison.

This is the Phase 1 acceptance test: two runs from clean state must produce
the same curve within the tolerances pre-registered in ground-truth.yaml.
Error rate = 5xx share of /checkout responses per 10 s bucket, relative to
the fault timestamp in the run manifest. 4xx are client errors, not the
incident, and are excluded from the numerator by construction.
"""
import argparse
import json
import statistics
import sys
from datetime import datetime
from pathlib import Path

import yaml

RUNS = Path("data/scenarios/s1")
GT = Path("scenarios/s1-payment-regression/ground-truth.yaml")
BUCKET = 10


def ts(s: str) -> datetime:
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def curve(run: Path):
    m = json.loads((run / "manifest.json").read_text())
    t_fault = ts(m["t_fault"])
    buckets: dict[int, list[int]] = {}
    for line in (run / "logs" / "gateway.jsonl").read_text().splitlines():
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        if r.get("route") != "/checkout" or "status" not in r:
            continue
        b = int((ts(r["ts"]) - t_fault).total_seconds() // BUCKET) * BUCKET
        d = buckets.setdefault(b, [0, 0])
        d[0] += 1
        d[1] += r["status"] >= 500
    pts = sorted((b, n, e / n if n else 0.0) for b, (n, e) in buckets.items())
    return m, pts


def evaluate(m, pts, gt):
    er = gt["expected_error_rate"]
    warm = -(m["steady_secs"] + m["lead_secs"]) + 20          # skip startup warm-up
    pre = [r for b, n, r in pts if warm <= b < -BUCKET and n >= 5]
    post = [r for b, n, r in pts if 20 <= b < m["post_secs"] - BUCKET and n >= 5]
    ok_pre = bool(pre) and max(pre) <= er["pre_fault_max"]
    mean_post = statistics.mean(post) if post else 0.0
    ok_post = bool(post) and er["post_fault"]["min"] <= mean_post <= er["post_fault"]["max"]
    return {"pre_max": max(pre) if pre else None, "pre_windows": len(pre),
            "post_mean": mean_post, "post_min": min(post) if post else None,
            "post_max": max(post) if post else None, "post_windows": len(post),
            "ok": ok_pre and ok_post}


def show(run: Path, m, pts, ev):
    print(f"\n== {run.name}  (fast={m['fast']}, seed={m['seed']}, rate={m['rate']}, "
          f"fault={m['fault_deploy']['deploy_id']} at {m['t_fault']})")
    print(f"   {'t-fault':>8}  {'reqs':>5}  {'5xx%':>6}")
    for b, n, r in pts:
        bar = "#" * int(r * 50)
        mark = " <- fault" if b == 0 else ""
        print(f"   {b:>+8}s  {n:>5}  {100*r:>5.1f}  {bar}{mark}")
    print(f"   pre-fault max {100*(ev['pre_max'] or 0):.1f}% over {ev['pre_windows']} windows | "
          f"post-fault mean {100*ev['post_mean']:.1f}% "
          f"[{100*(ev['post_min'] or 0):.1f}..{100*(ev['post_max'] or 0):.1f}] over {ev['post_windows']} windows "
          f"-> {'PASS' if ev['ok'] else 'FAIL'}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--compare", nargs="*", metavar="RUN_DIR",
                    help="two run dirs (default: the two most recent)")
    ap.add_argument("run", nargs="?", help="single run dir to print")
    a = ap.parse_args()
    gt = yaml.safe_load(GT.read_text())

    if a.compare is not None:
        runs = [Path(p) for p in a.compare] if a.compare else sorted(RUNS.iterdir())[-2:]
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
        print(f"\nrun-to-run post-fault drift {100*drift:.1f} pts (tolerance 5.0) -> "
              f"{'same' if same else 'DIFFERENT'}")
        print(f"\nS1 REPRODUCIBILITY: {'PASS' if ok else 'FAIL'}")
        sys.exit(0 if ok else 1)

    run = Path(a.run) if a.run else sorted(RUNS.iterdir())[-1]
    m, pts = curve(run)
    show(run, m, pts, evaluate(m, pts, gt))


if __name__ == "__main__":
    main()
