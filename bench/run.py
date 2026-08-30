#!/usr/bin/env python3
"""The benchmark runner (Phase 10): {conditions} x {scenarios} x repeats, unattended.

One cell = a fresh incident and one investigation:

  1. `just scenario <s>` -- clean state, stack up, the evidence engines
     restarted (their history is the scenario's alone), the fault injected on
     the fast timeline and left active
  2. `scripts/investigate.py --condition <c> --scenario <s> --approval allow --bench`
     -- the agent investigates; the gate's human is simulated as approving,
     so a wrong proposal EXECUTES and is scored as a wrong action (the
     conservative reading)
  3. the result file is left in bench/results/ whatever happened -- a
     harness error is a committed, `valid: false` run, and the cell is
     re-run up to --retries times on a fresh incident

Order (--order floor, the default): the pre-agreed floor first -- S1-S3 x
{baseline, spyglass}, one repeat of every cell before the next repeat, so an
interrupted run leaves balanced coverage -- then S6 x {baseline, spyglass},
then the ablation on S1-S3, then the ablation on S6. --order matrix runs
scenario-major. Nothing here is committed by the script: commit
bench/results/ yourself, every file, including failures.

  bench/run.py                                   # the whole matrix (~3.5 h)
  bench/run.py --scenarios s2 --conditions spyglass --repeats 1
  bench/run.py --dry-run                         # print the cells
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RESULTS = ROOT / "bench/results"
LOG = ROOT / ".local/bench.log"
FLOOR = ["s1", "s2", "s3"]
BASE = ["baseline", "spyglass"]
ABL = "ablation-no-novelty"


def say(msg: str) -> None:
    line = f"[bench {datetime.now(timezone.utc).strftime('%H:%M:%S')}] {msg}"
    print(line, flush=True)
    LOG.parent.mkdir(exist_ok=True)
    with LOG.open("a") as f:
        f.write(line + "\n")


def sh(cmd: list[str], env: dict | None = None, timeout: int = 1800) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, cwd=ROOT, env={**os.environ, **(env or {})}, capture_output=True, text=True, timeout=timeout)


def cells(scenarios: list[str], conditions: list[str], repeats: int, order: str) -> list[tuple[str, str, int]]:
    if order == "matrix":
        return [(s, c, r) for s in scenarios for c in conditions for r in range(1, repeats + 1)]
    out: list[tuple[str, str, int]] = []
    phases = [
        ([s for s in scenarios if s in FLOOR], [c for c in conditions if c in BASE]),
        ([s for s in scenarios if s not in FLOOR], [c for c in conditions if c in BASE]),
        ([s for s in scenarios if s in FLOOR], [c for c in conditions if c not in BASE]),
        ([s for s in scenarios if s not in FLOOR], [c for c in conditions if c not in BASE]),
    ]
    for ss, cs in phases:
        for r in range(1, repeats + 1):
            for s in ss:
                for c in cs:
                    out.append((s, c, r))
    return out


def harness_up() -> bool:
    try:
        urllib.request.urlopen("http://localhost:8790/api/v1/models", timeout=5).read()
        return True
    except Exception:
        return False


def run_cell(s: str, c: str, r: int, retries: int) -> dict:
    for attempt in range(1, retries + 2):
        say(f"{s}/{c} r{r} attempt {attempt}: fresh incident")
        p = sh(["just", "scenario", s], env={"SCENARIO_FAST": "1"}, timeout=1200)
        if p.returncode != 0:
            say(f"  scenario injection FAILED (exit {p.returncode}): {p.stderr[-400:]}")
            continue
        run_dir = p.stdout.strip().splitlines()[-1] if p.stdout.strip() else "?"
        say(f"  injected: {run_dir}")
        # Let the engines see the post-fault minute the injector observed (they tail live; this is a margin).
        time.sleep(3)
        say(f"  investigating: {c}")
        q = sh([sys.executable, "scripts/investigate.py", "--condition", c, "--scenario", s,
                "--approval", "allow", "--tag", f"bench-r{r}", "--bench"], timeout=2400)
        out = q.stdout
        m = re.search(r"result: (bench/results/\S+\.json)", out)
        path = ROOT / m.group(1) if m else None
        if q.returncode not in (0, 3) or not path or not path.exists():
            say(f"  investigate FAILED (exit {q.returncode}); stdout tail: {out[-500:]} stderr: {q.stderr[-500:]}")
            if path and path.exists():
                say(f"  kept: {path.relative_to(ROOT)}")
            continue
        res = json.loads(path.read_text())
        summary = [l for l in out.splitlines() if l.startswith("==") or "verification" in l or "rollbacks executed" in l or "tokens" in l]
        for l in summary[:5]:
            say("  " + l.strip())
        say(f"  -> {res['outcome']} valid={res['valid']} file={path.name}")
        if res["valid"]:
            return res
        say(f"  invalid run kept ({res.get('invalid_reason')}); retrying on a fresh incident")
    return {"outcome": "gave_up", "valid": False}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--scenarios", default="s1,s2,s3,s6")
    ap.add_argument("--conditions", default="baseline,spyglass,ablation-no-novelty")
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--order", choices=["floor", "matrix"], default="floor")
    ap.add_argument("--retries", type=int, default=2, help="re-runs of a cell whose run was invalid (harness error)")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--skip-setup", action="store_true", help="do not re-register MCP servers and agents first")
    a = ap.parse_args()
    plan = cells(a.scenarios.split(","), a.conditions.split(","), a.repeats, a.order)
    print(f"{len(plan)} cells:")
    for s, c, r in plan:
        print(f"  {s:3} {c:22} r{r}")
    if a.dry_run:
        return
    if not harness_up():
        sys.exit("TrueForge is not answering on :8790 (scripts/trueforge.sh start)")
    if not a.skip_setup:
        say("mcp servers up + tf-setup (SOPs and conditions as on disk)")
        p = sh(["scripts/mcp.sh", "start"])
        say("  " + p.stdout.strip().replace("\n", " | "))
        p = sh([sys.executable, "scripts/tf-setup.py"])
        if p.returncode != 0:
            sys.exit(f"tf-setup failed: {p.stderr[-800:]}")
        say("  " + p.stdout.strip().replace("\n", " | ")[:600])
    t0 = time.monotonic()
    done: list[tuple[str, str, int, str]] = []
    for i, (s, c, r) in enumerate(plan, 1):
        say(f"=== cell {i}/{len(plan)}: {s} / {c} / repeat {r}  (elapsed {(time.monotonic() - t0) / 60:.0f} min)")
        res = run_cell(s, c, r, a.retries)
        done.append((s, c, r, res.get("outcome", "?")))
    say(f"=== done: {len(plan)} cells in {(time.monotonic() - t0) / 60:.0f} min")
    for s, c, r, o in done:
        say(f"  {s:3} {c:22} r{r}: {o}")
    say("next: python3 bench/report.py ; git add bench/results ; commit every file")


if __name__ == "__main__":
    main()
