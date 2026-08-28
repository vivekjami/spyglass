#!/usr/bin/env python3
"""Minimal TrueForge REST driver.

Phase 0 item 6: prove that sessions can be created and turns executed
non-interactively, including approving a gated tool call, because Phase 10 runs
up to 54 benchmark investigations unattended. The spec named
@truefoundry/trueforge-sdk for this, but npm publishes that package as a
placeholder ("Do not use"), so we target the documented REST API instead.

This is deliberately dependency-free (stdlib only) and small enough to read.
The real bench runner grows from here.
"""
from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.request

BASE = os.environ.get("TRUEFORGE_URL", "http://localhost:8790").rstrip("/")


def _req(method: str, path: str, body: dict | None = None, timeout: float = 300.0):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(
        f"{BASE}/api/v1{path}", data=data, method=method,
        headers={"content-type": "application/json", "accept": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read().decode()
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"{method} {path} -> {e.code}: {e.read().decode()[:800]}") from None
    return json.loads(raw) if raw.strip() else {}


def create_session(spec: dict) -> str:
    """Create a session from an inline agent spec. Returns session id."""
    return _req("POST", "/sessions", {"agent": {"spec": spec}})["data"]["id"]


def turn(session_id: str, items: list[dict], timeout: float = 300.0,
         poll: float = 1.0) -> dict:
    """Start a turn and block until it stops running.

    `items` are TurnInputItems: user messages, or approval resumes. The API
    forbids mixing the two in one call.

    stream=False returns the *running* turn immediately, so we poll. A turn that
    stops on `tool.approval_required` is not an error — it is the gate doing its
    job, and the caller resumes it with an approval item.
    """
    t = _req("POST", f"/sessions/{session_id}/turns",
             {"input": items, "stream": False}, timeout)["data"]
    tid = t["id"]
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        t = get_turn(session_id, tid)
        # Status lives at state.status, not at the top level. Reading the wrong
        # key yields None forever, which polls until the heat death of the loop.
        if status(t) != "running":
            return t
        time.sleep(poll)
    raise TimeoutError(f"turn {tid} still running after {timeout}s")


def pending_approval(t: dict) -> tuple[str, str] | None:
    """Return (thread_id, tool_call_id) if this turn is parked on a gate.

    A gated turn finishes with status "done" and output None, carrying the ask
    in state.required_actions -- it does not sit in a "waiting" status. Reading
    only the status would make a blocked turn look like a completed one.
    """
    for ra in (t.get("state") or {}).get("required_actions") or []:
        if ra.get("type") == "tool.approval_required":
            for tc in ra.get("tool_calls", []):
                return ra.get("thread_id"), tc.get("id")
    return None


def events(session_id: str, turn_id: str) -> list[dict]:
    return _req("GET", f"/sessions/{session_id}/turns/{turn_id}/events")["data"]


def get_turn(session_id: str, turn_id: str) -> dict:
    return _req("GET", f"/sessions/{session_id}/turns/{turn_id}")["data"]


def status(t: dict) -> str:
    return (t.get("state") or {}).get("status", "unknown")


def output_text(t: dict) -> str:
    return ((t.get("state") or {}).get("output") or {}).get("content", "")


def usage(t: dict) -> dict:
    """Usage of the turn's FINAL model call only -- see usage_total().

    Measured in Phase 0: a turn with a sub-agent made three model calls
    (main 1184/126, sub-thread 221/41, main 1340/30) and state.output.usage
    reported just the last one. Do not feed this into the benchmark.
    """
    return ((t.get("state") or {}).get("output") or {}).get("usage", {})


def usage_total(evs: list[dict]) -> dict:
    """Token accounting for benchmark metrics 6-8: sum every model call in the
    turn, across all threads (main and every sub-agent thread)."""
    tot = {"input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0,
           "model_calls": 0, "threads": set()}
    for e in evs:
        u = e.get("usage")
        if e.get("type") == "model.message" and u:
            for k in ("input_tokens", "output_tokens", "cache_read_tokens"):
                tot[k] += u.get(k, 0) or 0
            tot["model_calls"] += 1
            tot["threads"].add(e.get("thread_id"))
    tot["threads"] = sorted(str(x) for x in tot["threads"])
    return tot


def tool_calls(evs: list[dict]) -> list[str]:
    """Benchmark metric 5. With preload:true this counts only the agent's own
    tool use; without it, harness list_tools/get_tool_info calls leak in."""
    return [tc["function"]["name"] for e in evs if e.get("type") == "model.message"
            for tc in (e.get("tool_calls") or [])]


def turn_with_retry(session_id: str, items: list[dict], attempts: int = 6,
                    base_wait: float = 20.0, timeout: float = 300.0) -> dict:
    """Run a turn, retrying when the provider rate-limits us.

    TrueForge does NOT retry a 429 itself: the turn transitions straight to
    status "error" and the trajectory is lost. On a rate-limited key that makes
    any multi-step investigation a coin flip, so the runner has to own the
    retry. A rate-limited run is discarded and re-run, never averaged in --
    a truncated trajectory is not a measurement of anything.
    """
    last = None
    for i in range(attempts):
        t = turn(session_id, items, timeout=timeout)
        last = t
        if status(t) != "error":
            return t
        wait = base_wait * (i + 1)
        print(f"    [retry {i+1}/{attempts}] turn errored; waiting {wait:.0f}s",
              flush=True)
        time.sleep(wait)
    return last


def user_message(text: str) -> dict:
    return {"type": "user.message", "content": text}


def approval(thread_id: str, tool_call_id: str, allow: bool, reason: str | None = None) -> dict:
    decision = {"status": "allow"} if allow else {"status": "deny"}
    if not allow and reason:
        decision["reason"] = reason
    return {"type": "user.tool_approval", "thread_id": thread_id,
            "tool_call_id": tool_call_id, "approval": decision}


def summarize(evs: list[dict]) -> None:
    """Print a compact trace: what the agent said, called, and waited on."""
    for e in evs:
        t = e.get("type", "?")
        if t == "model.message.delta":
            continue
        if t == "tool.call":
            print(f"  [tool.call] {e.get('name')} {json.dumps(e.get('arguments'))[:160]}")
        elif t == "tool.result":
            print(f"  [tool.result] {json.dumps(e.get('result'))[:200]}")
        elif t == "tool.approval_required":
            for tc in e.get("tool_calls", []):
                print(f"  [APPROVAL REQUIRED] thread={e.get('thread_id')} "
                      f"tool_call_id={tc.get('id')} name={tc.get('name')}")
        elif t in ("model.message", "model.message.done"):
            c = e.get("content")
            if c:
                print(f"  [model] {str(c)[:400]}")
        else:
            print(f"  [{t}]")
