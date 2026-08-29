"""Minimal MCP streamable-HTTP client for the Spyglass engine, stdlib only.

Used by the check scripts (ledger re-check, changepoint acceptance) so they
talk to the engine exactly the way the harness does -- through the MCP tool
surface, never a side door.
"""
from __future__ import annotations

import json
import urllib.request

ENGINE = "http://localhost:8791/mcp"
_H = {"content-type": "application/json", "accept": "application/json, text/event-stream"}


def _post(body: dict, sid: str | None = None, url: str = ENGINE) -> tuple[str | None, str]:
    h = dict(_H)
    if sid:
        h["mcp-session-id"] = sid
    req = urllib.request.Request(url, data=json.dumps(body).encode(), headers=h, method="POST")
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.headers.get("mcp-session-id", sid), r.read().decode()


def session(url: str = ENGINE, name: str = "spyglass-script") -> str:
    sid, _ = _post({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                               "clientInfo": {"name": name, "version": "1"}}}, url=url)
    _post({"jsonrpc": "2.0", "method": "notifications/initialized"}, sid, url=url)
    return sid


def call(sid: str, name: str, args: dict, url: str = ENGINE) -> dict:
    """Call a tool; return the parsed `{result, meta, ledger_n}` body. Raises on tool error."""
    _, raw = _post({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                    "params": {"name": name, "arguments": args}}, sid, url=url)
    data = [json.loads(l[6:]) for l in raw.splitlines() if l.startswith("data: ") and l[6:].strip()]
    res = data[-1]["result"]
    if res.get("isError"):
        raise RuntimeError(res["content"][0]["text"][:400])
    return json.loads(res["content"][0]["text"])


def wait_ready(sid: str, url: str = ENGINE, max_secs: float = 120.0) -> dict:
    """Block until the engine has read every source file to its end at least
    once (`caught_up`): after a restart the store is rebuilt from the logs,
    and a query issued mid-rebuild sees a partial world."""
    import time
    t0 = time.monotonic()
    r: dict = {}
    while time.monotonic() - t0 < max_secs:
        r = call(sid, "freshness_watermark", {}, url=url)["result"]
        if r.get("caught_up"):
            return r
        time.sleep(0.5)
    raise RuntimeError(f"engine did not catch up within {max_secs}s: {r}")
