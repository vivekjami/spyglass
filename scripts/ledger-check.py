#!/usr/bin/env python3
"""Re-check a ledger: re-execute every deterministic entry against the live
engine with its recorded (resolved) arguments and compare result digests.

This is what makes an evidence citation re-checkable (ADR-004, ADR-009): the
ledger stores the resolved window, so the same query over the same frozen
data must produce the same digest. Evidence ids are excluded from digests
because they are assigned per investigation. Exit 1 on any mismatch.

  scripts/ledger-check.py ledger/<investigation>.jsonl
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_client import call, session  # noqa: E402


def replay_args(entry: dict) -> dict:
    """Turn the ledger's resolved args back into the tool's argument shape."""
    a = dict(entry["args"])
    tool = entry["tool"]
    if tool in ("search_logs", "deploy_events", "novel_templates", "detect_changepoints") and a.get("window"):
        a["window"] = {"from": a["window"]["from"], "to": a["window"]["to"]}
    if tool in ("novel_templates", "detect_changepoints") and a.get("baseline"):
        a["baseline"] = {"from": a["baseline"]["from"], "to": a["baseline"]["to"]}
    if tool == "error_delta":
        for k in ("window_a", "window_b"):
            a[k] = {"from": a[k]["from"], "to": a[k]["to"]}
    return {k: v for k, v in a.items() if v is not None}


def check(path: Path) -> tuple[int, int, int]:
    entries = [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
    sid = session()
    ok = bad = skipped = 0
    print(f"{'n':>3} {'tool':20} {'det':3} {'recorded':16} {'replayed':16} verdict")
    for e in entries:
        if not e.get("deterministic"):
            skipped += 1
            print(f"{e['n']:>3} {e['tool']:20} no  {e['result_digest'][:16]:16} {'-':16} skipped (temporal)")
            continue
        if e["tool"] == "get_evidence":
            skipped += 1  # eids are per investigation; cannot be dereferenced from a new session
            print(f"{e['n']:>3} {e['tool']:20} yes {e['result_digest'][:16]:16} {'-':16} skipped (session-scoped)")
            continue
        try:
            resp = call(sid, e["tool"], replay_args(e))
            got = resp["meta"]["result_digest"]
        except Exception as ex:
            bad += 1
            print(f"{e['n']:>3} {e['tool']:20} yes {e['result_digest'][:16]:16} {'ERROR':16} {str(ex)[:60]}")
            continue
        same = got[:16] == e["result_digest"][:16]
        ok += same
        bad += not same
        print(f"{e['n']:>3} {e['tool']:20} yes {e['result_digest'][:16]:16} {got[:16]:16} {'match' if same else 'MISMATCH'}")
    return ok, bad, skipped


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    ok, bad, skipped = check(Path(sys.argv[1]))
    print(f"\nledger re-check: {ok} match, {bad} mismatch, {skipped} skipped -> {'PASS' if bad == 0 else 'FAIL'}")
    sys.exit(1 if bad else 0)
