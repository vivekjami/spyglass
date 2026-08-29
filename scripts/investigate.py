#!/usr/bin/env python3
"""Run one instrumented investigation against the live (faulted) stack.

The seed of the Phase 10 benchmark runner. One condition, one scenario, one
run: open a session on the condition's named agent, post the alert, drive the
approval gate per --approval, and record everything the benchmark scores --
tool calls, model calls, tokens summed across every thread, bytes the tools
returned to the context, wall time, approvals, whether a rollback executed,
and the error rate after the agent finished. The full event trace is kept so
a number can always be traced back to what happened.

  scripts/investigate.py --condition baseline --scenario s1 --approval ask

--approval ask   print each proposal and wait for y/n on stdin (the filmed run;
                 the gate is also visible in the TrueForge UI)
--approval allow auto-approve (unattended benchmark runs)
--approval deny  refuse with a reason (tests the report-only path)
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import tf  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
RESULTS = ROOT / "bench/results"
MAX_TURNS = 8
LINE = re.compile(r'^requests_total\{([^}]*)\}\s+([0-9.e+]+)$', re.M)


def dotenv(key: str, default: str = "") -> str:
    if os.environ.get(key):
        return os.environ[key]
    for line in (ROOT / ".env").read_text().splitlines() if (ROOT / ".env").exists() else []:
        if line.startswith(f"{key}="):
            return line.split("=", 1)[1].strip()
    return default


def now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def error_rate(seconds: float = 20.0) -> dict:
    """5xx share of gateway /checkout over a short window, from /metrics deltas."""
    url = f"http://127.0.0.1:{dotenv('GATEWAY_PORT', '8080')}/metrics"

    def scrape():
        txt = urllib.request.urlopen(url, timeout=3).read().decode()
        tot = err = 0.0
        for labels, val in LINE.findall(txt):
            if 'route="/checkout"' in labels:
                tot += float(val)
                err += float(val) if 'status="5' in labels else 0.0
        return tot, err

    a = scrape()
    time.sleep(seconds)
    b = scrape()
    dt, de = b[0] - a[0], b[1] - a[1]
    return {"window_secs": seconds, "requests": dt, "errors": de, "rate": (de / dt) if dt else None}


def journal() -> list[dict]:
    p = ROOT / "data/deploy/journal.jsonl"
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()] if p.exists() else []


def tool_bytes(evs: list[dict]) -> int:
    return sum(len(json.dumps(e.get("content"))) for e in evs if e.get("type") == "tool.response")


def proposal_of(evs: list[dict], tool_call_id: str) -> dict:
    for e in evs:
        if e.get("type") == "model.message":
            for tc in e.get("tool_calls") or []:
                if tc.get("id") == tool_call_id:
                    return {"name": tc["function"]["name"], "arguments": tc["function"].get("arguments")}
    return {"name": "?", "arguments": None}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--condition", required=True, help="bench/conditions/<name>.json")
    ap.add_argument("--scenario", default="s1")
    ap.add_argument("--approval", choices=["ask", "allow", "deny"], default="ask")
    ap.add_argument("--alert", help="override the alert text (default: the watcher's message)")
    ap.add_argument("--tag", default="", help="free-text label stored in the result")
    a = ap.parse_args()

    cond = json.loads((ROOT / "bench/conditions" / f"{a.condition}.json").read_text())
    agent_name = cond["name"]
    alert = a.alert or ("payments error alert firing -- gateway /checkout 5xx rate elevated above the 5% threshold "
                        "for 2 consecutive windows. Investigate; roll back if a deploy caused it.")
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_path = RESULTS / f"{a.scenario}-{a.condition}-{run_id}.json"
    RESULTS.mkdir(parents=True, exist_ok=True)

    journal_before = journal()
    versions_before = json.loads((ROOT / "data/deploy/current.json").read_text())
    pre_rate = error_rate(10)
    print(f"[{a.condition}/{a.scenario}] agent={agent_name} pre-run 5xx={pre_rate['rate']:.1%}  "
          f"payments={versions_before['payments']['version']}", flush=True)

    sid = tf._req("POST", "/sessions", {"agent": {"name": agent_name}})["data"]["id"]  # by saved-agent name
    print(f"session {sid}  ->  {tf.BASE}/sessions/{sid}", flush=True)
    t_start = time.monotonic()
    started_at = now()

    turns, all_events, approvals = [], [], []
    items = [tf.user_message(alert)]
    final_text, outcome = "", "incomplete"
    for turn_no in range(1, MAX_TURNS + 1):
        t = tf.turn(sid, items, timeout=1500)
        evs = tf.events(sid, t["id"])
        all_events.extend(evs)
        turns.append({"turn_id": t["id"], "status": tf.status(t), "events": len(evs),
                      "tool_calls": tf.tool_calls(evs), "usage": tf.usage_total(evs)})
        print(f"  turn {turn_no}: status={tf.status(t)} tool_calls={len(tf.tool_calls(evs))} "
              f"tokens={tf.usage_total(evs)['input_tokens']}/{tf.usage_total(evs)['output_tokens']}", flush=True)
        if tf.status(t) == "error":
            outcome = "harness_error"
            break
        pend = tf.pending_approval(t)
        if not pend:
            final_text = tf.output_text(t)
            outcome = "completed"
            break
        thread, tcid = pend
        prop = proposal_of(evs, tcid)
        print(f"  *** APPROVAL REQUIRED: {prop['name']} {prop['arguments']}", flush=True)
        if a.approval == "ask":
            ans = input("  approve? [y/N] ").strip().lower()
            allow, reason = ans == "y", "Operator declined."
        elif a.approval == "allow":
            allow, reason = True, None
        else:
            allow, reason = False, "Operator declined: report-only run."
        approvals.append({"turn": turn_no, "proposal": prop, "decision": "allow" if allow else "deny", "at": now()})
        print(f"  -> {'APPROVED' if allow else 'DENIED'}", flush=True)
        items = [tf.approval(thread, tcid, allow, reason)]
    else:
        outcome = "max_turns"

    wall = time.monotonic() - t_start
    post_rate = error_rate(20)
    journal_after = journal()
    new_entries = journal_after[len(journal_before):]
    versions_after = json.loads((ROOT / "data/deploy/current.json").read_text())
    rollbacks = [e for e in new_entries if e["kind"] == "rollback"]
    totals = tf.usage_total(all_events)
    calls = tf.tool_calls(all_events)

    result = {
        "scenario": a.scenario, "condition": a.condition, "agent": agent_name, "run_id": run_id, "tag": a.tag,
        "model": None, "started_at": started_at, "finished_at": now(),
        "alert": alert, "approval_policy": a.approval,
        "outcome": outcome,
        "metrics": {
            "wall_time_secs": round(wall, 1),
            "turns": len(turns),
            "tool_calls": len(calls),
            "tool_calls_by_name": {n: calls.count(n) for n in sorted(set(calls))},
            "model_calls": totals["model_calls"],
            "threads": totals["threads"],
            "input_tokens": totals["input_tokens"],
            "output_tokens": totals["output_tokens"],
            "total_tokens": totals["input_tokens"] + totals["output_tokens"],
            "cache_read_tokens": totals["cache_read_tokens"],
            "tool_response_bytes": tool_bytes(all_events),
            "approvals": approvals,
            "rollbacks_executed": [{"service": e["service"], "to": e["version"], "from": e.get("from_version"),
                                    "deploy_id": e.get("deploy_id"), "eids": e.get("justification_eids", [])} for e in rollbacks],
            "journal_entries_added": new_entries,
            "versions_before": {k: v["version"] for k, v in versions_before.items()},
            "versions_after": {k: v["version"] for k, v in versions_after.items()},
            "error_rate_pre_run": pre_rate,
            "error_rate_post_run": post_rate,
        },
        "final_output": final_text,
        "turns": turns,
        "events": all_events,
    }
    # Ledger (Spyglass conditions): the engine writes ledger/<mcp-session-id>.jsonl.
    engine_sid = next((s.get("session_id") for e in all_events if e.get("type") == "mcp.initialize"
                       for s in e.get("mcp_servers", []) if s.get("name") == "spyglass-engine"), None)
    if engine_sid:
        lp = ROOT / "ledger" / f"{engine_sid}.jsonl"
        entries = [json.loads(l) for l in lp.read_text().splitlines() if l.strip()] if lp.exists() else []
        issued = sorted({eid for en in entries for eid in en.get("eids", [])}, key=lambda x: int(x[1:]))
        cited = sorted({m for m in re.findall(r"\bE\d+\b", final_text)}, key=lambda x: int(x[1:]))
        recheck = None
        if entries:
            import subprocess
            p = subprocess.run([sys.executable, str(ROOT / "scripts/ledger-check.py"), str(lp)], capture_output=True, text=True)
            recheck = {"exit": p.returncode, "verdict": p.stdout.strip().splitlines()[-1] if p.stdout else p.stderr[-300:]}
        result["ledger"] = {"investigation": engine_sid, "path": str(lp.relative_to(ROOT)), "entries": len(entries),
                            "eids_issued": len(issued), "eids_cited_in_rca": cited,
                            "eids_cited_valid": [c for c in cited if c in issued],
                            "engine_latency_ms": [en["latency_ms"] for en in entries],
                            "recheck": recheck, "ledger_entries": entries}
        result["metrics"]["evidence_citations"] = len([c for c in cited if c in issued])
    try:
        result["model"] = tf._req("GET", "/agents")["data"]
        result["model"] = next(x["manifest"]["model"]["name"] for x in result["model"] if x["name"] == agent_name)
    except Exception:
        pass
    out_path.write_text(json.dumps(result, indent=2))

    m = result["metrics"]
    print(f"\n== {a.condition}/{a.scenario} {run_id}: {outcome} ==")
    print(f"   wall {m['wall_time_secs']}s | turns {m['turns']} | tool calls {m['tool_calls']} {m['tool_calls_by_name']}")
    print(f"   model calls {m['model_calls']} on {len(m['threads'])} thread(s) | tokens {m['input_tokens']} in / {m['output_tokens']} out "
          f"| tool bytes to context {m['tool_response_bytes']:,}")
    print(f"   rollbacks executed: {m['rollbacks_executed'] or 'none'} | payments {m['versions_before']['payments']} -> {m['versions_after']['payments']}")
    print(f"   5xx before {pre_rate['rate']:.1%} -> after {post_rate['rate']:.1%}" if post_rate["rate"] is not None else "   (no post traffic)")
    if result.get("ledger"):
        L = result["ledger"]
        lat = L["engine_latency_ms"]
        print(f"   ledger: {L['path']} | {L['entries']} entries, {L['eids_issued']} eids issued, {len(L['eids_cited_valid'])} cited in RCA "
              f"| engine latency p50 {sorted(lat)[len(lat)//2] if lat else 0:.2f} ms max {max(lat) if lat else 0:.2f} ms | re-check: {(L['recheck'] or {}).get('verdict','-')}")
    print(f"   result: {out_path.relative_to(ROOT)}")
    # The acceptance bar is "digests re-check": a mismatch is a loud failure,
    # after the result is written so the evidence of it is kept.
    rc = (result.get("ledger") or {}).get("recheck") or {}
    if rc.get("exit", 0) != 0:
        print("   LEDGER RE-CHECK FAILED", file=sys.stderr)
        sys.exit(3)


if __name__ == "__main__":
    main()
