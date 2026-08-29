#!/usr/bin/env python3
"""Register Spyglass's MCP servers and benchmark-condition agents in TrueForge.

Idempotent: MCP servers are create-or-replace; agents are created or updated
by name. Run after `just mcp-up`, and again whenever a condition file or an
SOP changes. Conditions live in bench/conditions/*.json; the `$MODEL_A` /
`$MODEL_B` placeholders and `instructions_file` are resolved here.
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import tf  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
MCP_SERVERS = {
    "spyglass-engine": ("http://localhost:8791/mcp",
                        "Spyglass evidence engine (read-only): bounded, template-grouped, evidence-id-stamped tools -- search_logs, error_delta, deploy_events, freshness_watermark, get_evidence, service_topology."),
    "spyglass-rawtools": ("http://localhost:8793/mcp",
                          "BASELINE tools: raw, unshaped telemetry access (tail_logs, grep_logs, get_metric, list_services, deploy_events)."),
    "spyglass-deployer": ("http://localhost:8792/mcp",
                          "The ONE mutating action: rollback (approval-required), plus read-only current_versions."),
}


def dotenv(key: str, default: str = "") -> str:
    if os.environ.get(key):
        return os.environ[key]
    for line in (ROOT / ".env").read_text().splitlines() if (ROOT / ".env").exists() else []:
        if line.startswith(f"{key}="):
            return line.split("=", 1)[1].strip()
    return default


def resolve_model(ref: str) -> str:
    """'$MODEL_A' -> the catalog name TrueForge serves (dashed), by model_id or name."""
    want = dotenv(ref.lstrip("$")) if ref.startswith("$") else ref
    models = tf._req("GET", "/models")["data"]
    for m in models:
        if m.get("model_id") == want or m.get("name", "").split("/")[-1] == want.replace(".", "-") or m.get("name") == want:
            return m["name"]
    sys.exit(f"model '{want}' not served by the harness; have: {[m['name'] for m in models]}")


def register_mcp_servers() -> None:
    for name, (url, desc) in MCP_SERVERS.items():
        tf._req("PUT", "/settings/mcp-servers",
                {"manifest": {"type": "remote", "name": name, "url": url, "description": desc}})
        tools = [t["name"] for t in tf._req("GET", f"/mcp-servers/{name}/tools")["data"]]
        print(f"mcp  {name:20} {url:32} tools={tools}")


def register_agents() -> None:
    existing = {a["name"]: a["id"] for a in tf._req("GET", "/agents")["data"]}
    for path in sorted((ROOT / "bench/conditions").glob("*.json")):
        c = json.loads(path.read_text())
        manifest = {
            "model": {"name": resolve_model(c["model"]["name"])},
            "instructions": (ROOT / c["instructions_file"]).read_text(),
            "mcp_servers": c["mcp_servers"],
            "config": c["config"],
        }
        name = c["name"]
        if name in existing:
            tf._req("PUT", f"/agents/{existing[name]}", {"manifest": manifest})  # update body is manifest-only
            verb = "updated"
        else:
            tf._req("POST", "/agents", {"name": name, "manifest": manifest})
            verb = "created"
        gated = [s.get("require_approval_for_tools") for s in c["mcp_servers"] if s.get("require_approval_for_tools")]
        print(f"agent {name:20} {verb:8} model={manifest['model']['name']} gated={gated} "
              f"iteration_limit={c['config']['iteration_limit']}")


if __name__ == "__main__":
    register_mcp_servers()
    register_agents()
