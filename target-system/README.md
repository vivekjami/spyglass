# target-system

The synthetic production system the incidents happen in. Scenery, not the
show — but the evidence it emits is the engine's raw material, so its shape is
specified. Full description: [docs/architecture.md → Target system](../docs/architecture.md#target-system-phase-1--built).

```
loadgen ──▶ gateway ──▶ orders ──▶ payments-v1   (known good)
                            ╲──▶ payments-v2   (S1 regression)   both ALWAYS on
                            ╲──▶ postgres
                            ╰──  /deploy/current.json  (routing; written by the deployer)
```

One Docker image (`Dockerfile`), five instances selected by Compose `command`.

| Package | Role | Port |
|---|---|---|
| `common/` | JSON logging, Prometheus metrics, `x-request-id` propagation, deploy-state lookup, deterministic noise | — |
| `gateway/` | edge; request capture (sanitized header subset, body ≤ 1 KB, never auth headers) | 8080 |
| `orders/` | postgres insert, then charge via the payments version `current.json` names | 8081 |
| `payments/` | `SERVICE_VERSION=v1` good · `v2` seeded S1 regression + two benign novel INFO decoys | 8082 (v1), 8083 (v2) on the host |
| `loadgen/` | seeded traffic: 20% non-USD, 2% malformed, 1% injection-styled user-agents | — |

## Evidence contract

**Logs** — `data/logs/<instance>.jsonl` and stdout, one JSON object per line:

| Always | On request completion | On upstream calls | On exceptions | Gateway capture |
|---|---|---|---|---|
| `ts service instance version level req_id msg` | `route status latency_ms deploy_id` | `upstream upstream_version` | `stack` (≤ 2 KB) | `kind=request_capture method path headers body` |

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

## Adding a fault

A new seeded regression is a branch on `SERVICE_VERSION` in the service's
handler, a version added to the deployer's `KNOWN_VERSIONS`, a Compose instance
if it must coexist with the old one, and a scenario directory with
pre-registered ground truth (`scenarios/SCHEMA.md`).
