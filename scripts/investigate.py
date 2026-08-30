#!/usr/bin/env python3
"""Run one instrumented investigation against the live (faulted) stack.

One condition, one scenario, one run: open a session on the condition's named
agent, post the scenario's alert (from its ground-truth.yaml), drive the
approval gate per --approval, and record everything the benchmark scores --
tool calls, model calls, tokens summed across every thread, bytes the tools
returned to the context, wall time, approvals, whether a rollback executed,
the edge error rate and p95 latency after the agent finished, the engine's
verification verdict, and the ledger. The full event trace is kept so a
number can always be traced back to what happened; bench/report.py scores
from these files alone. bench/run.py calls this once per cell.

  scripts/investigate.py --condition baseline --scenario s1 --approval ask

--approval ask   print each proposal and wait for y/n on stdin (the filmed run;
                 the gate is also visible in the TrueForge UI)
--approval allow auto-approve (unattended benchmark runs)
--approval deny  refuse with a reason (tests the report-only path)
"""
from __future__ import annotations

import argparse
import glob
import hashlib
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
HIST = re.compile(r'^latency_ms_bucket\{([^}]*)\}\s+([0-9.e+]+)$', re.M)
DEFAULT_ALERT = ("payments error alert firing -- gateway /checkout 5xx rate elevated above the 5% threshold "
                 "for 2 consecutive windows. Investigate; roll back if a deploy caused it.")


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
    """The edge over a short window, from /metrics deltas: 5xx share of gateway
    /checkout and its p95 latency (from the histogram buckets). The runner's
    own measurement of the world after the agent finished -- the same for
    every condition, independent of what the agent claims."""
    url = f"http://127.0.0.1:{dotenv('GATEWAY_PORT', '8080')}/metrics"

    def scrape():
        txt = urllib.request.urlopen(url, timeout=3).read().decode()
        tot = err = 0.0
        for labels, val in LINE.findall(txt):
            if 'route="/checkout"' in labels:
                tot += float(val)
                err += float(val) if 'status="5' in labels else 0.0
        hist = {}
        for labels, val in HIST.findall(txt):
            if 'route="/checkout"' in labels:
                le = re.search(r'le="([^"]+)"', labels)
                if le:
                    hist[le.group(1)] = hist.get(le.group(1), 0.0) + float(val)
        return tot, err, hist

    a = scrape()
    time.sleep(seconds)
    b = scrape()
    dt, de = b[0] - a[0], b[1] - a[1]
    p95 = None
    deltas = {k: b[2].get(k, 0.0) - a[2].get(k, 0.0) for k in b[2]}
    total = deltas.get("+Inf", 0.0)
    if total > 0:
        for le in sorted((k for k in deltas if k != "+Inf"), key=float):
            if deltas[le] >= 0.95 * total:
                p95 = float(le)
                break
        p95 = p95 if p95 is not None else float("inf")
    return {"window_secs": seconds, "requests": dt, "errors": de, "rate": (de / dt) if dt else None,
            "p95_latency_ms_le": p95 if p95 != float("inf") else ">5000"}


def scenario_ground_truth(scenario: str) -> dict:
    hits = sorted(glob.glob(str(ROOT / f"scenarios/{scenario}-*/ground-truth.yaml")))
    if not hits:
        return {}
    try:
        import yaml
        return yaml.safe_load(Path(hits[0]).read_text()) or {}
    except Exception:
        return {}


def scenario_run_manifest(scenario: str) -> dict | None:
    """The injector's manifest of the latest run of this scenario (absolute
    fault time etc.), embedded so the scorer needs nothing outside the result."""
    runs = sorted((ROOT / "data/scenarios" / scenario).glob("*/manifest.json"))
    if not runs:
        return None
    try:
        return json.loads(runs[-1].read_text())
    except (OSError, json.JSONDecodeError):
        return None


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()[:16]


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


# The evidence engines by their registered MCP server name (scripts/mcp.sh, scripts/tf-setup.py).
ENGINE_URLS = {"spyglass-engine": "http://localhost:8791/mcp", "spyglass-engine-ablation": "http://localhost:8794/mcp"}


def engine_server_of(evs: list[dict]) -> tuple[str | None, str | None]:
    """(mcp session id, server name) of the evidence engine this run used."""
    for e in evs:
        if e.get("type") != "mcp.initialize":
            continue
        for s in e.get("mcp_servers", []):
            if str(s.get("name", "")).startswith("spyglass-engine"):
                return s.get("session_id"), s.get("name")
    return None, None


def engine_session_of(evs: list[dict]) -> str | None:
    return engine_server_of(evs)[0]


def ledger_entries(engine_sid: str | None) -> list[dict]:
    lp = ROOT / "ledger" / f"{engine_sid}.jsonl" if engine_sid else None
    return [json.loads(l) for l in lp.read_text().splitlines() if l.strip()] if lp and lp.exists() else []


def render_gate(prop: dict, evs: list[dict]) -> None:
    """What the human sees at the gate: the restated proposal, and each cited
    evidence id resolved to the ledger line that produced it (Phase 9)."""
    args = prop.get("arguments") or {}
    if isinstance(args, str):
        try:
            args = json.loads(args)
        except json.JSONDecodeError:
            args = {"raw": args}
    print(f"  *** APPROVAL REQUIRED: {prop['name']}", flush=True)
    for k in ("proposal_id", "service", "to_version", "expected_current"):
        if k in args:
            print(f"      {k:16} {args[k]}", flush=True)
    eids = args.get("justification_eids") or []
    entries = ledger_entries(engine_session_of(evs))
    by_eid = {eid: en for en in entries for eid in en.get("eids", [])}
    print(f"      justification   {eids}", flush=True)
    for eid in eids:
        en = by_eid.get(eid)
        print(f"        {eid:4} {en['tool'] + ': ' + en['summary'][:110] if en else 'NOT ISSUED BY THE ENGINE IN THIS INVESTIGATION'}", flush=True)
    for k, v in args.items():
        if k not in ("proposal_id", "service", "to_version", "expected_current", "justification_eids"):
            print(f"      {k:16} {v}", flush=True)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--condition", required=True, help="bench/conditions/<name>.json")
    ap.add_argument("--scenario", default="s1")
    ap.add_argument("--approval", choices=["ask", "allow", "deny"], default="ask")
    ap.add_argument("--alert", help="override the alert text (default: the watcher's message)")
    ap.add_argument("--tag", default="", help="free-text label stored in the result")
    ap.add_argument("--bench", action="store_true", help="mark the run as a benchmark run (bench/report.py aggregates only these)")
    a = ap.parse_args()

    cond_path = ROOT / "bench/conditions" / f"{a.condition}.json"
    cond = json.loads(cond_path.read_text())
    agent_name = cond["name"]
    gt = scenario_ground_truth(a.scenario)
    alert = a.alert or (gt.get("alert") or "").strip() or DEFAULT_ALERT
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
        render_gate(prop, all_events)
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
    journal_kinds = {k: sum(1 for e in new_entries if e["kind"] == k) for k in sorted({e["kind"] for e in new_entries})}
    totals = tf.usage_total(all_events)
    calls = tf.tool_calls(all_events)

    execs = [json.loads(tc["function"]["arguments"]).get("command") if isinstance(tc["function"].get("arguments"), str) else (tc["function"].get("arguments") or {}).get("command")
             for e in all_events if e.get("type") == "model.message" for tc in (e.get("tool_calls") or []) if tc["function"]["name"] == "exec"]
    result = {
        "scenario": a.scenario, "condition": a.condition, "agent": agent_name, "run_id": run_id, "tag": a.tag,
        "benchmark": bool(a.bench),
        "valid": outcome == "completed",
        "invalid_reason": None if outcome == "completed" else outcome,
        "model": None, "started_at": started_at, "finished_at": now(),
        "alert": alert, "approval_policy": a.approval,
        "ground_truth_version": gt.get("version"),
        "scenario_run": scenario_run_manifest(a.scenario),
        "provenance": {"condition_file": str(cond_path.relative_to(ROOT)), "condition_sha256": sha(cond_path),
                       "instructions_file": cond.get("instructions_file"),
                       "instructions_sha256": sha(ROOT / cond["instructions_file"]) if cond.get("instructions_file") else None},
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
            "journal_kinds_added": journal_kinds,
            "versions_before": {k: v["version"] for k, v in versions_before.items()},
            "versions_after": {k: v["version"] for k, v in versions_after.items()},
            "error_rate_pre_run": pre_rate,
            "error_rate_post_run": post_rate,
            "sandbox_exec_commands": execs,
        },
        "final_output": final_text,
        "turns": turns,
        "events": all_events,
    }
    # Ledger (Spyglass conditions): the engine writes ledger/<mcp-session-id>.jsonl.
    engine_sid = engine_session_of(all_events)
    if engine_sid:
        lp = ROOT / "ledger" / f"{engine_sid}.jsonl"
        entries = ledger_entries(engine_sid)
        # Verification (C11) is judged by the engine; the benchmark reads its verdict here, not the prose.
        checks = [en for en in entries if en["tool"] == "verify_recovery"]
        closing = [en for en in entries if en["tool"] == "verified_recovery"]
        escal = [en for en in entries if en["tool"] == "escalation"]
        result["verification"] = {"checks": len(checks), "closed": bool(closing), "escalated": bool(escal),
                                  "verdict": (closing or escal or [{}])[0].get("summary"),
                                  "last_check": checks[-1]["summary"] if checks else None}
        issued = sorted({eid for en in entries for eid in en.get("eids", [])}, key=lambda x: int(x[1:]))
        cited = sorted({m for m in re.findall(r"\bE\d+\b", final_text)}, key=lambda x: int(x[1:]))
        recheck = None
        if entries:
            import subprocess
            # Re-check against the engine that issued the entries (the ablation engine digests differently: P10 F6e).
            engine_url = ENGINE_URLS.get(engine_server_of(all_events)[1] or "", ENGINE_URLS["spyglass-engine"])
            p = subprocess.run([sys.executable, str(ROOT / "scripts/ledger-check.py"), str(lp), "--engine", engine_url], capture_output=True, text=True)
            recheck = {"exit": p.returncode, "verdict": p.stdout.strip().splitlines()[-1] if p.stdout else p.stderr[-300:]}
        result["ledger"] = {"investigation": engine_sid, "engine": engine_url if entries else None, "path": str(lp.relative_to(ROOT)), "entries": len(entries),
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
        V = result.get("verification") or {}
        print(f"   verification (engine-judged): {V.get('checks', 0)} check(s); {'CLOSED' if V.get('closed') else 'ESCALATED' if V.get('escalated') else 'NOT CLOSED'} -- {V.get('verdict') or V.get('last_check') or 'no checks'}")
    print(f"   journal kinds added: {m['journal_kinds_added']}")
    print(f"   result: {out_path.relative_to(ROOT)}")
    # The acceptance bar is "digests re-check": a mismatch is a loud failure,
    # after the result is written so the evidence of it is kept.
    rc = (result.get("ledger") or {}).get("recheck") or {}
    if rc.get("exit", 0) != 0:
        print("   LEDGER RE-CHECK FAILED", file=sys.stderr)
        sys.exit(3)


if __name__ == "__main__":
    main()
