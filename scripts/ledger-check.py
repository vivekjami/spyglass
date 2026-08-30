#!/usr/bin/env python3
"""Re-check a ledger: re-execute every deterministic entry against the live
engine with its recorded (resolved) arguments and compare result digests.

This is what makes an evidence citation re-checkable (ADR-004, ADR-009): the
ledger stores the resolved window, so the same query over the same frozen
data must produce the same digest. Evidence ids are excluded from digests
because they are assigned per investigation. Exit 1 on any mismatch.

  scripts/ledger-check.py ledger/<investigation>.jsonl [--engine http://localhost:8794/mcp]

The entries must be replayed against the engine that issued them: the
ablation engine (:8794) runs the same binary with novelty disabled and
`w_n = 0`, so its bundles and watermarks digest differently from the main
engine's (Phase 10 F6e). Default: --engine http://localhost:8791/mcp, or
$SPYGLASS_ENGINE_URL.
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp_client import ENGINE, call, session  # noqa: E402


def replay_args(entry: dict) -> dict:
    """Turn the ledger's resolved args back into the tool's argument shape."""
    a = dict(entry["args"])
    tool = entry["tool"]
    if tool in ("search_logs", "deploy_events", "novel_templates", "detect_changepoints", "build_evidence_bundle", "get_exemplar_request") and a.get("window"):
        a["window"] = {"from": a["window"]["from"], "to": a["window"]["to"]}
    if tool in ("novel_templates", "detect_changepoints") and a.get("baseline"):
        a["baseline"] = {"from": a["baseline"]["from"], "to": a["baseline"]["to"]}
    if tool == "build_evidence_bundle":
        a.pop("baseline", None)  # derived from the window; the recorded weights replay as overrides
        a["weights"] = {k: v for k, v in (a.get("weights") or {}).items() if k.startswith("w_")}
    if tool == "error_delta":
        for k in ("window_a", "window_b"):
            a[k] = {"from": a[k]["from"], "to": a[k]["to"]}
    return {k: v for k, v in a.items() if v is not None}


def check(path: Path, engine: str = ENGINE) -> tuple[int, int, int]:
    entries = [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
    sid = session(url=engine)
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
            resp = call(sid, e["tool"], replay_args(e), url=engine)
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
    args = sys.argv[1:]
    engine = os.environ.get("SPYGLASS_ENGINE_URL", ENGINE)
    if "--engine" in args:
        i = args.index("--engine")
        engine = args[i + 1]
        del args[i:i + 2]
    if len(args) != 1:
        sys.exit(__doc__)
    print(f"engine: {engine}")
    ok, bad, skipped = check(Path(args[0]), engine)
    print(f"\nledger re-check: {ok} match, {bad} mismatch, {skipped} skipped -> {'PASS' if bad == 0 else 'FAIL'}")
    sys.exit(1 if bad else 0)
