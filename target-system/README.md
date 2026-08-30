# target-system

The synthetic production system the incidents happen in. Scenery, not the
show — but the evidence it emits is the engine's raw material, so its shape is
specified. Full description: [docs/architecture.md → Target system](../docs/architecture.md#target-system-phase-1--built).

```
loadgen ──▶ gateway ──▶ orders ──▶ payments-v1   (known good)
                            ╲──▶ payments-v2   (S1 regression)   both ALWAYS on
                            ╲──▶ postgres
                            ╲──▶ fraudcheck    (external vendor: UNOBSERVED -- no logs, no metrics, no host port)
                            ╰──  /deploy/current.json  (routing + config release; written by the deployer)
                                 /knobs/<name>.json    (environment changes without a change event; written by scenarios)
```

One Docker image (`Dockerfile`), six instances selected by Compose `command`.

| Package | Role | Port |
|---|---|---|
| `common/` | JSON logging, Prometheus metrics, `x-request-id` propagation, deploy-state lookup, deterministic noise | — |
| `gateway/` | edge; request capture (sanitized header subset, body ≤ 1 KB, never auth headers) | 8080 |
| `orders/` | postgres insert, synchronous fraud pre-check (fails open, never logged), then charge via the payments version `current.json` names; orders' own routed version selects its **config**: `v1`/`v1.1` vendor API v1 + 5 s timeout, `v1.2` vendor API v2 + 10 s timeout (S2, config-only release) | 8081 |
| `payments/` | `SERVICE_VERSION=v1` good · `v2` seeded S1 regression + two benign novel INFO decoys; a 2 % steady-state cache hiccup (ERROR `cache write failed: TimeoutError`, retried — the same level as the real failure, because the engine keys templates by level) and a **fail-closed** path on a real cache-write failure (503, ERROR `cache write failed: <error class>` + `redis memory pressure …` WARN with the store's numbers) — S3 | 8082 (v1), 8083 (v2) on the host |
| `fraudcheck/` | the external vendor, from inside: `/v1/score` ~3 ms; `/v2/score` 1.5 s, 9 s for premium/corporate; a knob degrades any share of calls (S6). Not in `data/logs`, not scraped, not published | — |
| `loadgen/` | seeded traffic: 20% non-USD, 2% malformed, 1% injection-styled user-agents | — |

## Evidence contract

**Logs** — `data/logs/<instance>.jsonl` and stdout, one JSON object per line:

| Always | On request completion | On upstream calls | On exceptions | Gateway capture |
|---|---|---|---|---|
| `ts service instance version level req_id msg` | `route status latency_ms deploy_id` (+ `replay=<experiment>` when the request was the engine's causal-check replay) | `upstream upstream_version` | `stack` (≤ 2 KB) | `kind=request_capture method path headers body` |

**Metrics** — Prometheus text at `/metrics` on every service:
`requests_total{service,route,status}` · `errors_total{service,route}` (status ≥ 500) ·
`latency_ms_bucket{service,route}` · `upstream_requests_total{service,upstream,version,status}`.
`/health` and `/metrics` are excluded from logs and metrics.

## Environment

| Variable | Default | Meaning |
|---|---|---|
| `SERVICE_NAME` / `INSTANCE_NAME` / `SERVICE_VERSION` | — | identity stamped on every log line |
| `PORT` | per service | listen port inside the container |
| `SPYGLASS_LOG_DIR` | `/var/log/spyglass` | bind-mounted to `./data/logs` |
| `SPYGLASS_DEPLOY_STATE` | `/deploy/current.json` | bind-mounted read-only from `./data/deploy` |
| `LOADGEN_SEED` / `LOADGEN_RATE` | `42` / `10` | the request stream is a pure function of the seed |
| `LOADGEN_MALFORMED_RATE` / `LOADGEN_INJECTION_RATE` | `0.02` / `0.01` | background noise rates |
| `GATEWAY_PORT` … `PAYMENTS_V2_PORT` | 8080–8083 | **host** ports; override in `.env` when taken |

Containers run as the host user (`SPYGLASS_UID/GID`, exported by `just`) so
`data/logs` stays deletable without sudo.

redis runs `maxmemory 64mb` with `maxmemory-policy noeviction`: an
idempotency record must never vanish silently, so a write into a full store
fails loudly instead. That is the property scenario S3 exercises.

## Knobs (Phase 10)

`./data/knobs/<name>.json`, mounted read-only at `/knobs`, read per request
and cached on mtime (`common.knob`). A knob is how a scenario changes the
*environment* without a change event — the gateway's latency blip
(`gateway.json: {"blip_ms": 400, "blip_share": 0.5}`, S2's decoy) and the
vendor's degradation (`fraudcheck.json: {"degrade": {"share": 0.12,
"latency_ms": 9000}}`, S6). Knobs are not telemetry: no tool in either
benchmark condition reads the directory and the services never log them.
`just clean` removes them.

## Adding a fault

A new seeded regression is a branch on `SERVICE_VERSION` in the service's
handler (or a config entry keyed by the routed version, as orders `v1.2`), a
version added to the deployer's `KNOWN_VERSIONS`, a Compose instance if it
must coexist with the old one, and a scenario directory with pre-registered
ground truth (`scenarios/SCHEMA.md`). A fault that is not a change event is
an injector step against the environment — a knob, a redis command — and its
ground truth says `change: null`.
