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
import urllib.request
from pathlib import Path

ENGINE = "http://localhost:8791/mcp"
H = {"content-type": "application/json", "accept": "application/json, text/event-stream"}


def post(body: dict, sid: str | None = None) -> tuple[str | None, str]:
    h = dict(H)
    if sid:
        h["mcp-session-id"] = sid
    req = urllib.request.Request(ENGINE, data=json.dumps(body).encode(), headers=h, method="POST")
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.headers.get("mcp-session-id", sid), r.read().decode()


def session() -> str:
    sid, _ = post({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                   "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                              "clientInfo": {"name": "ledger-check", "version": "1"}}})
    post({"jsonrpc": "2.0", "method": "notifications/initialized"}, sid)
    return sid


def call(sid: str, name: str, args: dict) -> dict:
    _, raw = post({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": name, "arguments": args}}, sid)
    data = [json.loads(l[6:]) for l in raw.splitlines() if l.startswith("data: ") and l[6:].strip()]
    res = data[-1]["result"]
    if res.get("isError"):
        raise RuntimeError(res["content"][0]["text"][:300])
    return json.loads(res["content"][0]["text"])


def replay_args(entry: dict) -> dict:
    """Turn the ledger's resolved args back into the tool's argument shape."""
    a = dict(entry["args"])
    tool = entry["tool"]
    if tool in ("search_logs", "deploy_events") and a.get("window"):
        a["window"] = {"from": a["window"]["from"], "to": a["window"]["to"]}
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
