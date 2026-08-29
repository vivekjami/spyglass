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
| 1 | TrueForge launches and a model responds | ✅ **PASS** (F0) |
| 2 | A Rust `rmcp` HTTP MCP server connects and its tools are invocable | ✅ **PASS** — connect, enumerate, invoke, and agent-driven invoke (F3) |
| 3 | Sandbox can reach a Docker Compose container | ❌ **FAIL, fallback written** — isolated by design at two layers; harness allowlist hard-coded (F9) |
| 4 | Subagents spawn and return | ✅ **PASS** — own thread, events visible on parent turn (F11) |
| 5 | A tool marked approval-required actually gates | ✅ **PASS** — live gate; programmatic allow **and** deny (F4) |
| 6 | *(added)* Programmatic session control for the benchmark runner | ✅ PASS |
| 7 | *(added)* Benchmark-fairness flags identified and pinned | ✅ PASS (ADR-016; extended by F12) |
| — | **Provider key viability** | ✅ **resolved** — key rotated to a paid tier; 0 × 429 since (F10) |

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

### F0. Harness launches and a model responds ✅ (item 1)

`scripts/configure-model.sh` registered `google-gemini` through
`POST /api/v1/settings/model-providers`. A session with an inline spec, one
turn, and a poll of `GET /sessions/{id}/turns/{tid}` returned:

```
status : done
output : PHASE0_ITEM1_OK
usage  : {"input_tokens": 1144, "output_tokens": 85, "cache_read_tokens": 0,
          "input_tokens_breakdown": {"harness": 1288, "skills": 0,
          "instructions": 7, "tool_definitions": 0, "messages": 0}}
```

`input_tokens_breakdown` is a gift for the benchmark: it separates harness
overhead from instructions, tool definitions, and messages, so the evidence
payload's token cost can be isolated from the harness's fixed cost per call.

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

**Consequences:**
- No Daytona account, no cloud dependency, no API key for the sandbox.
- Skills also became available (they require a sandbox).
- A judge reproducing the demo needs `bwrap`, `socat`, `rg` — now documented and
  scripted.
- **But a local sandbox is not local network reach.** The first draft of this
  finding assumed it was. The direct test in F9 shows the sandbox is
  network-isolated by design and TrueForge's egress allowlist is hard-coded.
  Item 3 is a **FAIL with a written fallback** — see F9.

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

`crates/phase0-probe` (a throwaway crate, removed in Phase 3 once the real engine existed — see git history) was an `rmcp` 3.1.4
streamable-HTTP server exposing `probe_ping` (read-only) and `probe_rollback`
(simulated mutation). TrueForge registered it and enumerated both tools with the
`schemars`-derived JSON Schema intact, including per-field descriptions:

```
GET /api/v1/mcp-servers/phase0-probe/tools
  probe_ping      {message: string}                 preload: true
  probe_rollback  {service: string, to_version: string}  preload: true
```

Direct `tools/call` over the streamable-HTTP transport executes and returns the
engine's response contract intact:

```
POST /mcp  {"method":"tools/call","params":{"name":"probe_ping",
                                            "arguments":{"message":"phase0 item2"}}}
-> {"content":[{"type":"text","text":
     "{\"echo\":\"phase0 item2\",\"engine_latency_ms\":0.004749,
       \"ok\":true,\"source\":\"phase0-probe (rust/rmcp)\"}"}],
    "isError":false}
```

**`engine_latency_ms: 0.004749`** — 4.7 microseconds round-trip inside the tool.
That is the ADR-002 Rust argument made empirical rather than asserted, and it is
the number the demo puts on screen at 0:45–1:30.

Driven by an agent in a live session (`mcp_servers: [{name: "phase0-probe"}]`),
the model called the tool itself and reported `engine_latency_ms: 0.005727`.

The highest-risk integration seam — Rust `rmcp` against TrueForge's
`@modelcontextprotocol/sdk` 1.29 — is fully crossed: connect, enumerate,
invoke, and agent-driven invoke.

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

**Live test — PASS.** With `require_approval_for_tools: ["probe_rollback"]`:

1. The turn finished with `state.status: "done"`, `output: null`, and
   `state.required_actions: [{type: "tool.approval_required", thread_id: "main",
   tool_calls: [{id: "call_487988", ...}]}]`. **The tool did not execute.**
   Note the shape: a gated turn does not sit in a "waiting" status — it
   completes, and the ask travels in `required_actions`. A runner that reads
   only the status will mistake a blocked turn for a finished one.
2. Resuming with `{"type":"user.tool_approval","thread_id":"main",
   "tool_call_id":"call_487988","approval":{"status":"allow"}}` as the sole
   input item executed the tool (`"executed":true,"simulated":true`) and the
   agent reported success.

3. **Deny — PASS.** Resuming with `{"status":"deny","reason":"Phase 0 deny-path
   test: rollback refused by operator."}`: the tool did **not** execute; the
   agent received it as a tool *error* —
   `{"error":"User denied tool call: Phase 0 deny-path test: …"}` — and
   reported the refusal verbatim. The reason reaches the model intact, which
   is what the S6 refuse/escalate scenario needs.

With `preload: true` on the MCP server, the only call before the gate was
`probe_rollback` — no `list_tools`/`get_tool_info`. F12's fix confirmed.

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
the runner needs no credentials.

**Verified live.** [`scripts/tf.py`](../scripts/tf.py) (stdlib only) does
create-session → create-turn → poll → events → usage. Three API facts the docs
do not state, each of which cost a wrong first draft:

- A turn's status is at **`state.status`**, not top-level `status`. Reading the
  wrong key returns `None` forever, and a poll loop never exits.
- `stream: false` returns the *running* turn; completion is by polling
  `GET /sessions/{id}/turns/{tid}` until `state.status != "running"`.
- A gated turn ends as `done` with `output: null` and the ask in
  `state.required_actions` (F4). `CreateTurnRequest.stream` defaults to `true`
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

**Selected for this build:** Model A = `google-gemini/gemini-3.6-flash`,
Model B = `google-gemini/gemini-3.1-pro-preview`. Flash is the cheapest capable
option in the catalog, which matters because the baseline condition is designed
to burn tokens — over-investigation is the phenomenon under study, and ADR-016
deliberately removes the harness's compaction safety net. Cost per run is what
buys repeats beyond n=3.

**Watch item:** a cheaper model raises the risk that the baseline fails *so*
early that the Phase 2 foil footage is uninformative (the demo's 0:10–0:30
segment depends on the baseline visibly drowning, not instantly erroring). If
that happens, the fix is to raise Model A, not to weaken the treatment — and it
gets recorded, per ADR-012.

### F9. The local sandbox cannot reach Compose — by design, at two layers ❌ (item 3)

Tested **directly against the sandbox runtime, with no model involved**. The
question is about networking; routing it through an LLM on a rate-limited key
was the wrong experiment, and it cost three failed attempts before that was
obvious. Target: a one-container Compose service at `172.22.0.2:8899`, also
port-published on `127.0.0.1:8899`, serving a sentinel string.

| Test (`srt -c "curl …"`) | Result |
|---|---|
| default settings → `172.22.0.2:8899` | `rc=7` — blocked |
| default settings → `localhost:8899` | `rc=7` — blocked |
| allowlist includes the IP → `172.22.0.2:8899` | `rc=7` — **still blocked** |
| allowlist includes `example.com` → `http://example.com` | `200` — proxy path works |
| allowlist includes the IP **and** `NO_PROXY` unset → `172.22.0.2:8899` | **`200`, sentinel received** |
| same via `localhost:8899` | **`200`, sentinel received** |

Layer by layer:

1. **SRT removes the network namespace entirely on Linux.** The only egress is
   an HTTP/SOCKS proxy on the host, reached over a bind-mounted Unix socket
   (`HTTP_PROXY=http://srt.<token>@localhost:3128`). Direct connections fail
   with `Network is unreachable`.
2. **SRT sets `NO_PROXY` to every private range** —
   `localhost,127.0.0.1,::1,169.254.0.0/16,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16`.
   Compose networks live in `172.16.0.0/12`, so `curl` bypasses the proxy for
   them and hits layer 1. Deliberate: it stops sandboxed code reaching the
   host's LAN and loopback services.
3. **The proxy will forward to a private IP** when the destination is on the
   allowlist and the client is told to use the proxy — the last two rows prove
   the mechanism end-to-end.
4. **TrueForge's allowlist is a hard-coded constant** with no env, settings-file,
   or API override. `LOCAL_SANDBOX_ALLOWED_DOMAINS` = `pypi.org`, `*.pypi.org`,
   `pythonhosted.org`, `files.pythonhosted.org`, `*.pythonhosted.org`,
   `github.com`, `api.github.com`, `codeload.github.com`, `*.github.com`,
   `githubusercontent.com`, `raw.githubusercontent.com`,
   `objects.githubusercontent.com`, `*.githubusercontent.com`. Package installs
   and repo fetches; nothing else.

Under TrueForge, agent-generated code in the sandbox **cannot reach the Compose
stack**, and there is no supported knob to change that. Patching the harness
bundle is not an option: not reproducible on a judge's machine, and exactly the
"rebuild sponsor infrastructure" anti-pattern the spec rules out.

**Fallbacks, in order of preference — decision needed before Phase 8.**
*Decided at Phase 8: (A) built; measured v1 0/20 vs v2 20/20 on S1;
[ADR-010](adr/ADR-010-sandbox-verification-before-action.md) amended.*

- **(A) Replay as a bounded MCP tool — recommended.** Move the experiment into
  the evidence plane: `replay_exemplar(template_id, versions[], n)` on the
  engine (or a small third "experiment" MCP server), which sits on the Compose
  network and reaches `payments-v1` / `payments-v2` directly. The agent still
  designs the experiment (which exemplar, which versions, N) and still receives
  `{v1: k₁/N, v2: k₂/N}` as ledger entries E5/E6. Survives: the
  correlation→causation upgrade, the demo's intellectual peak, determinism,
  bounds, ledger citation. Lost: the sandbox as the actor, and the
  "agent-written code runs in the sponsor's sandbox" story. ADR-010 is amended,
  not reversed — the controlled experiment happens; its executor changes.
- **(B) Daytona + a public tunnel.** The spec's original path. Needs a Daytona
  key, a tunnel (cloudflared/ngrok) exposing both payments services, and
  TrueForge's Daytona provider — three external dependencies live during a
  demo. Keeps the sandbox load-bearing. Not primary on risk; kept if the
  sponsor-tool story is judged worth it.
- **(C) Deploy-window bisection, RCA downgraded to correlational.** The spec's
  stated fallback. Strictly weaker than (A) — it gives up causal evidence — so
  it is now the fallback *to the fallback*.

The sandbox keeps a real job under (A): computing and presenting the replay
statistics, generating the postmortem, and analysing ledger data — code
execution that needs no network.

### F10. The free-tier Gemini key is unusable for this project ❌ BLOCKER

Every test after item 5 failed on:

```
Quota exceeded for metric: generativelanguage.googleapis.com/generate_content_free_tier_requests,
limit: 20, model: gemini-3.6-flash
quotaId: "GenerateRequestsPerDayPerProjectPerModel-FreeTier"
```

**20 requests per day** for `gemini-3.6-flash`, on top of a 5-per-minute limit
seen earlier. The "please retry in 14s" hint is misleading — it reports the
per-minute window while the per-day bucket is empty.

Why this is a blocker, not an inconvenience:

- One agent turn is not one request. With deferred tool loading (the default)
  the trace for a *single* tool use was `list_tools → get_tool_info →
  call_tool → answer`: **four model calls**. A real investigation is 20–50.
- Phase 10 is {baseline, spyglass, ablation} × {S1..S3} × 3 = 27–54
  investigations. On this key that is weeks, not a Sunday morning.
- **TrueForge does not retry a 429.** The turn goes straight to
  `state.status: "error"` and the trajectory is gone. A rate-limited run is
  not a slow run; it is a lost run. `scripts/tf.py` has `turn_with_retry` for
  the runner, but it re-runs the whole turn — it cannot resume a dead one.

**Resolved the same evening.** The key was rotated to a paid-tier credential
(`scripts/configure-model.sh` now uses `PUT` = create-or-replace, so a rotation
is one command). A single-call probe returned `QUOTA_OK`; items 4 and 5-deny
then ran back-to-back in parallel with zero 429s. The model choice stands.

The lesson is recorded because it will bite a judge too: the README's "any
provider TrueForge supports" now says *on a paid tier*.

### F11. Subagents spawn, run on their own thread, and return ✅ (item 4)

With `dynamic_sub_agents.enabled: true` and no MCP servers attached, the lead
called the built-in and got the result back:

```
create_sub_agent {"name":"subagent_test","input":"Reply with exactly the text SUBAGENT_ALIVE and nothing else."}
  thread.created   thread=01ba1c2f…
  model.message    thread=01ba1c2f…   usage={"input_tokens":221,"output_tokens":41}
  thread.done      thread=01ba1c2f…
output: SUBAGENT_ALIVE
```

Facts that matter for the SOP and the benchmark:

- **Signature:** `create_sub_agent(name, input)`. The lead writes the sub-agent's
  entire brief as `input`. So the three analysts are *text the SOP tells the
  lead to generate*, exactly as F5 predicted — and the budget line goes in
  that text.
- **Sub-agent events are on the parent turn's stream** with their own
  `thread_id`, bracketed by `thread.created` / `thread.done`. The ledger writer
  can attribute every tool call to a thread. Sub-agent tool calls will also be
  visible there (none in this test — no tools attached).
- **Token accounting trap.** The turn's `state.output.usage` is the usage of
  the *final* model call only — here `1340/30`, when the turn actually made
  three calls (main `1184/126`, sub-thread `221/41`, main `1340/30`; total
  **2745 in / 197 out**). A runner that reads the turn-level number would
  under-report tokens by ~2× on a turn with one trivial sub-agent, and far more
  on a real fan-out. `scripts/tf.py::usage_total()` sums `model.message.usage`
  across all threads; that is the only number that goes into metrics 6–8.

### F12. Deferred tool loading inflates the tool-call metric ⚠️

In every traced turn, before calling `probe_rollback` the agent first called the
built-ins `list_tools` and `get_tool_info`. TrueForge's *deferred tool loading*
keeps tool schemas out of the prompt until asked for — good for token cost on
large catalogs, but it adds 1–2 harness calls per tool the agent touches.

That lands on benchmark metric 5 (tool calls) and on tokens, and it would hit
the two conditions differently: the baseline has 5 raw tools, Spyglass has 10
shaped ones. The MCP server spec has `preload: true` / `preload_tools: [...]`
to inject schemas up front. **Pin `preload: true` in every condition** so the
count measures the agent's investigation, not the harness's discovery — added
to ADR-016 as the same class of confound.

**Confirmed:** with `preload: true`, the deny-path run's only call before the
gate was `probe_rollback` itself (F4). Metric 5 is clean under that setting.

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
2. **Sandbox** — the local sandbox-runtime path replaces Daytona (F1), **but
   it cannot reach Compose** (F9). Sandbox Causal Verification must be rewritten
   around fallback (A): the replay runs as a bounded MCP tool on the evidence
   plane; the sandbox does non-network work. ADR-010 amended accordingly.
3. **Bench runner** — the TS SDK is not a safe dependency; the runner targets
   the REST API (F6). Technology Stack updated accordingly.
4. **Benchmark conditions** — must pin `compaction` and `large_tool_response`
   in every condition (F7 / ADR-016), and say so in `docs/benchmark.md`.
5. **Prerequisites** — Node 22.14+, and `bwrap`/`socat`/`rg` for the sandbox,
   belong in the README's setup section.
6. **Model tier** — a free-tier key cannot run the benchmark (F10). The README's
   "any provider TrueForge supports" needs the qualifier *on a paid tier*.
7. **Tool preloading** — `preload: true` pinned in every condition (F12).
8. **Token accounting** — metrics 6–8 must sum `model.message.usage` across
   every thread of a turn; the turn-level `usage` is the last call only (F11).
