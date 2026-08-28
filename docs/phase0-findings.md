# Phase 0 — Harness validation findings

**Objective:** eliminate infrastructure uncertainty before writing project code.
TrueForge was released 2026-08-19 and every later phase rests on assumptions
about it. This document records what was verified, what diverged from the spec,
and a written fallback for anything that failed.

**Run date:** 2026-08-28 · **Host:** Linux 7.0.0-30-generic, x86_64
**Versions:** TrueForge `0.1.4` · rmcp `3.1.4` · Node `v22.23.2` · Rust `1.94.0`
· Docker `29.7.2` / Compose `v5.5.0`

**Acceptance bar** (from the spec): *all items demonstrated, or a written
fallback per failure.* A failed item with an armed fallback is a passing Phase 0.

---

## Status summary

| # | Item | Status |
|---|---|---|
| 1 | TrueForge launches and a model responds | ⏳ pending model API key |
| 2 | A Rust `rmcp` HTTP MCP server connects and its tools are invocable | ✅ handshake PASS · invocation pending key |
| 3 | Sandbox can reach a Docker Compose container | ✅ **unblocked** — local sandbox available (see F1) |
| 4 | Subagents spawn and return | ⏳ pending model API key |
| 5 | A tool marked approval-required actually gates | ✅ mechanism confirmed · live test pending key |
| 6 | *(added)* Programmatic session control for the benchmark runner | ✅ PASS |
| 7 | *(added)* Benchmark-fairness flags identified and pinned | ✅ PASS (ADR-016) |

---

## Environment prerequisites discovered

### P1. Node 22.14+ is required, and Ubuntu ships 20.x

TrueForge declares `engines: { node: ">=22" }` and its docs say "22.14 or newer".
The host had Node **v20.20.2** with no version manager installed. Nothing runs
until this is fixed.

**Resolution:** [`scripts/install-node22.sh`](../scripts/install-node22.sh)
fetches the official Node `v22.23.2` tarball, verifies it against
`SHASUMS256.txt`, and unpacks it to `~/.local/node-v22`. No system packages, no
shell-profile edits, no root. `source scripts/env.sh` puts it on `PATH`.
Reversible with `rm -rf ~/.local/node-v22`.

This is a prerequisite a judge's clean machine will also hit — it belongs in the
README setup section, not just here.

---

## Findings

### F1. The sandbox does **not** require Daytona — a local sandbox exists ✅

**This was the single biggest risk in the plan, and it resolved in our favour.**

The published docs state plainly: *"Daytona is the only sandbox provider
supported today."* Daytona is a **cloud** sandbox, which almost certainly cannot
reach a Docker Compose network on the developer's host — so Phase 0 item 3
looked doubtful *by construction*, threatening ADR-010's causal-replay step and
the demo's strongest segment.

The startup log said otherwise:

```
warn Local sandbox fallback is unavailable
  {"reason":"SRT host dependencies missing (linux: bwrap, socat, rg):
    bwrap: resolved=/usr/bin/bwrap; socat: not on PATH; rg: not on PATH"}
```

TrueForge bundles `@anthropic-ai/sandbox-runtime` and falls back to it when
three host binaries are present. `bwrap` ships with Ubuntu; `socat` and `rg` do
not, and `apt` needs root we do not have. Both were installed without root by
[`scripts/install-sandbox-deps.sh`](../scripts/install-sandbox-deps.sh) — a
prebuilt static `ripgrep` binary, and `socat` built from source into
`~/.local`. After a restart:

```
info Local sandbox fallback is available
  {"platform":"linux","shell":"/usr/bin/bash","python":"/usr/bin/python3.12"}
```

```json
GET /api/v1/capabilities
{"data":{"sandbox":{"enabled":true},"skill":{"enabled":true},"settings":{"enabled":true}}}
```

**Consequences, all favourable:**
- No Daytona account, no cloud dependency, no API key for the sandbox.
- The sandbox runs **on this host**, so reaching Compose services is ordinary
  local networking rather than an inbound-tunnel problem.
- Skills also became available (they require a sandbox).
- A judge reproducing the demo needs `bwrap`, `socat`, `rg` — now documented and
  scripted.

The deploy-window bisection fallback stays written down (ADR-010) but is no
longer the expected path.

### F2. MCP transport: `remote` only — no stdio ✅ (confirms ADR-003)

`MCPServerType` is an enum with exactly one member: `"remote"`. The manifest is:

```json
{"manifest": {
  "type": "remote",
  "name": "<resource-name>",
  "url": "http://host/mcp",
  "description": "<required, non-empty>",
  "auth": {"type": "header"|"dcr"}      // omit when no credentials needed
}}
```

There is **no stdio transport**, so an MCP server must be a long-lived HTTP
service. The spec chose streamable HTTP for independent reasons (ADR-003:
engine lifecycle decoupled from the harness); that choice is now also the only
one available. `POST /api/v1/settings/mcp-servers` registers one.

### F3. The Rust ↔ TypeScript MCP seam works ✅ (item 2)

[`crates/phase0-probe`](../crates/phase0-probe) is a throwaway `rmcp` 3.1.4
streamable-HTTP server exposing `probe_ping` (read-only) and `probe_rollback`
(simulated mutation). TrueForge registered it and enumerated both tools with the
`schemars`-derived JSON Schema intact, including per-field descriptions:

```
GET /api/v1/mcp-servers/phase0-probe/tools
  probe_ping      {message: string}                 preload: true
  probe_rollback  {service: string, to_version: string}  preload: true
```

The highest-risk integration seam — Rust `rmcp` against TrueForge's
`@modelcontextprotocol/sdk` 1.29 — is crossed. Tool *invocation from a session*
additionally requires a model, and is pending a key.

### F4. The approval gate is real, and can be driven programmatically ✅ (item 5)

The docs describe approvals only as a bullet point ("Pause before
write/destructive MCP tools"), with no mechanism. The OpenAPI schema has it, and
this matters far beyond the demo: **Phase 10 runs up to 54 benchmark
investigations unattended.** If approval required a human click, the benchmark
would be impossible.

Marking tools, per agent, per MCP server:

```json
"mcp_servers": [{
  "name": "spyglass-deployer",
  "require_approval_for_tools": ["@all" | "@write" | "@destructive" | "rollback"]
}]
```

The harness then emits, and accepts, these events:

```jsonc
// emitted by the harness — the turn stops here
{"type":"tool.approval_required","id":"<ulid>","created_at":"...",
 "thread_id":"<thread>","tool_calls":[{...}]}

// sent back as a turn input item to resume
{"type":"user.tool_approval","thread_id":"<thread>","tool_call_id":"<id>",
 "approval":{"status":"allow"}}
{"type":"user.tool_approval","thread_id":"<thread>","tool_call_id":"<id>",
 "approval":{"status":"deny","reason":"<shown to the agent>"}}
```

`CreateTurnRequest.input` warns: *"Do not mix user messages with approval or
tool-response items."* Deny carries an optional reason surfaced to the agent —
useful for the S6 refusal scenario.

**Live gating test is pending a model key.**

### F5. Subagents are dynamic, not statically defined ⚠️ spec divergence

The spec assumes three statically defined subagents (`agent/subagents/*.md`)
with per-subagent budgets, "enforced via TrueForge subagent config where
supported". It is **not supported**.

TrueForge instead ships a built-in `create_sub_agent` tool; the root agent
generates each subagent's instructions at runtime. The only knob is
`config.dynamic_sub_agents.enabled` (default `true`). The docs state:
*"Subagents have access to the same MCP tools and sandbox environment as the
root agent."* There is no per-subagent tool restriction and no budget field.

**Mitigation** (the spec's intent survives; the enforcement point moves):
- The three analyst roles become *instructions the SOP tells the lead to
  generate*, not static config files.
- Tool-call and token budgets are restated in the generated subagent
  instructions — advisory, and honestly labelled as such.
- **Real** enforcement moves to where it can be enforced: engine-side per-client
  rate limiting, plus `config.iteration_limit` (1–1024, default 100) at the lead
  level.

This is a documentation change to the spec, not a design failure — but the
README's claim of "per-subagent tool/token budgets" must be corrected rather
than quietly left standing.

### F6. Programmatic session control confirmed ✅ (item 6, added)

The spec's Technology Stack names `@truefoundry/trueforge-sdk` for the bench
runner. On npm, that package's `latest` (0.1.3) carries the description
*"Placeholder so trusted publishing can be configured. Do not use."* — while the
docs show a working SDK example. Rather than depend on resolving that
contradiction, the runner targets the REST API directly, which is fully
specified and served locally at `/api/v1/openapi.json`:

```
POST   /api/v1/sessions                              create session (inline agent manifest)
POST   /api/v1/sessions/{id}/turns                   create + execute a turn (SSE by default)
GET    /api/v1/sessions/{id}/turns/{tid}/events      turn events
GET    /api/v1/sessions/{id}/turns/{tid}/subscribe   subscribe to a running turn
POST   /api/v1/sessions/{id}/cancel                  cancel
GET,POST,PUT /api/v1/settings/mcp-servers            register MCP servers
GET,POST,PUT /api/v1/settings/model-providers        configure providers
GET    /api/v1/mcp-servers/{name}/tools              tool discovery
```

Local standalone mode runs with **auth disabled** (`warn Auth is disabled`), so
the runner needs no credentials. `CreateTurnRequest.stream` defaults to `true`
and can be set `false` to get the running turn immediately and poll events — the
simpler shape for an unattended benchmark.

### F7. The harness shapes tool responses by default — a benchmark confound ⚠️

Found in the agent manifest schema, and easy to miss:

```json
"config": {
  "context_management": {
    "compaction":          {"enabled": true, "trigger": {"type":"input_tokens","value": <int>}},
    "large_tool_response": {"enabled": true}
  }
}
```

**Both default to `true`.** `large_tool_response` means TrueForge *already
performs its own shaping of oversized tool results*.

Left at defaults, the BASELINE condition would receive harness-shaped raw
telemetry — the control group getting a weaker version of the treatment. That
silently contaminates ADR-012's "identical in everything but the evidence
interface," and would understate Spyglass's effect while making the comparison
indefensible if a judge asked.

**Decision: pin both flags explicitly, identically, in every condition — see
[ADR-016](adr/ADR-016-harness-context-management-pinning.md).**

### F8. Model providers available

`GET /api/v1/catalogs/model-providers` lists 8 providers plus `custom`
(any OpenAI-compatible endpoint):

| Provider | Models |
|---|---|
| `openai` | gpt-5.4-mini, gpt-5.5, gpt-5.6-luna, gpt-5.6-sol, gpt-5.6-terra |
| `anthropic` | claude-fable-5, claude-haiku-4-5, claude-opus-4-8, claude-opus-5, claude-sonnet-4-6, claude-sonnet-5 |
| `google-gemini` | gemini-3.1-pro-preview, gemini-3.6-flash |
| `fireworks` | deepseek-v4-pro, glm-5p2, kimi-k2p7-code, kimi-k3, minimax-m3 |
| `zai` | glm-5-turbo, glm-5.1, glm-5.2 |
| `moonshot` | kimi-k2.7-code, kimi-k3 |
| `alibaba` | qwen3.7-flash, qwen3.7-max, qwen3.7-plus, qwen3.8-max |
| `together` | DeepSeek-V4-Pro, MiniMax-M3, Kimi-K2.7-Code, Kimi-K3, Qwen3.7-* |
| `custom` | any OpenAI-compatible endpoint |

This makes the **Model Generalization Experiment** free of new code: Model A and
Model B are provider/model strings in `bench/conditions/`, exactly as the spec
required for the thesis to be testable.

---

## Reproducing this

```bash
scripts/install-node22.sh          # Node 22 -> ~/.local/node-v22 (no root)
scripts/install-sandbox-deps.sh    # bwrap/socat/rg for the LOCAL sandbox (no root)
source scripts/env.sh              # PATH + SQLITE_PATH + TRUEFORGE_URL
scripts/trueforge.sh start         # harness on :8790, state in .local/ (disposable)

cargo build --release -p phase0-probe
./target/release/phase0-probe &    # rmcp MCP server on :8791/mcp

curl -s -X POST localhost:8790/api/v1/settings/mcp-servers \
  -H 'content-type: application/json' \
  -d '{"manifest":{"type":"remote","name":"phase0-probe",
       "url":"http://localhost:8791/mcp","description":"Phase 0 probe."}}'

curl -s localhost:8790/api/v1/mcp-servers/phase0-probe/tools   # expect 2 tools
```

`rm -rf .local` returns the harness to first-run state.

---

## Spec revisions this phase forces

1. **Subagents** — the README's "per-subagent tool/token budgets enforced via
   TrueForge subagent config" is wrong. Rewrite per F5: dynamic subagents,
   advisory prompt budgets, real enforcement engine-side plus `iteration_limit`.
2. **Sandbox** — replace "Daytona / TrueForge sandbox" with the local
   sandbox-runtime path and its three host dependencies (F1). Bisection stays as
   the fallback, demoted from likely to unlikely.
3. **Bench runner** — the TS SDK is not a safe dependency; the runner targets
   the REST API (F6). Technology Stack updated accordingly.
4. **Benchmark conditions** — must pin `compaction` and `large_tool_response`
   in every condition (F7 / ADR-016), and say so in `docs/benchmark.md`.
5. **Prerequisites** — Node 22.14+, and `bwrap`/`socat`/`rg` for the sandbox,
   belong in the README's setup section.
