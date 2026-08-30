# scripts

| Script | What | Needs |
|---|---|---|
| `env.sh` | `source` it: Node 22 + `~/.local/bin` on PATH, `SQLITE_PATH` inside the repo, `TRUEFORGE_URL` | — |
| `install-node22.sh` | official Node 22 tarball, SHASUMS-verified, into `~/.local/node-v22` | no root |
| `install-sandbox-deps.sh` | `rg` (prebuilt) + `socat` (from source) into `~/.local`, for TrueForge's local sandbox | no root; `bwrap` from the distro |
| `install-just.sh` | `just` prebuilt into `~/.local/bin` | no root |
| `trueforge.sh` | `start / stop / status / logs` the harness; finds the PID by port, never `pkill -f` | env.sh |
| `configure-model.sh` | register (or rotate) the model provider from `.env`; idempotent (`PUT`) | `.env` with `MODEL_PROVIDER`, `MODEL_API_KEY` |
| `tf.py` | stdlib REST driver: sessions, turns, polling, approvals, cross-thread token totals — the seed of the bench runner | harness up |
| `watch.py` | error-rate dashboard + threshold alert; opens a TrueForge session on alert (`--no-session` to disable) | stack up; `.env` for `GATEWAY_PORT`, `SPYGLASS_AGENT` |
| `mcp.sh` | `start / stop / restart / status` the four MCP servers (engine :8791, deployer :8792, rawtools :8793, ablation engine :8794 = the same engine with `--ablation no-novelty`) by listening port | built binaries |
| `tf-setup.py` | register MCP servers + every `bench/conditions/*.json` as a named agent; idempotent (`PUT` for updates takes manifest only) | harness up |
| `mcp_client.py` | stdlib MCP streamable-HTTP client (`session`, `call`, `wait_ready`) — the check scripts talk to the engine through the tool surface, never a side door | engine up |
| `investigate.py` | one instrumented investigation: session on a named agent, the scenario's alert, approval policy, metrics + engine verdict + ledger + full event trace → `bench/results/`; runs the ledger re-check; `--bench` marks a benchmark run | harness + MCP servers up |
| `ledger-check.py` | re-execute every deterministic ledger entry against the live engine and compare digests (ADR-004) | engine up |
| `changepoint-check.py` | Phase 5 acceptance on the latest S1 run (`just s5-check`) | engine up |
| `bundle-check.py` | Phase 6/7 acceptance on the latest S1 run (`just s7-check`) | engine up |
| `replay-check.py` | Phase 8 acceptance on the latest S1 run (`just s8-check`): exemplar sanitized, replay proportions measured, ledger entries, negative control, no leakage | engine + stack up |
| `gate-check.py` | Phase 9 acceptance, live (`just s9-check`): double-fire, approve-after-manual-rollback, expired and restated proposals, engine-judged closure and escalation, the budget backstop; leaves payments at v1 | engine + deployer + stack up |
| `scenario-curve.py` | 5xx and p95-latency curve of a scenario run; `--scenario sN --compare` two runs against the ground truth's tolerances (`just scenario-check sN`) | run snapshots in `data/scenarios/<sN>/` |
| `s1-curve.py` | the Phase 1 name for `scenario-curve.py --scenario s1` (`just s1-check`) | 〃 |
| `validate-ground-truth.py` | check `ground-truth.yaml` files against `scenarios/SCHEMA.md` | PyYAML |

Facts these encode that the TrueForge docs do not state (Phase 0): a turn's
status is `state.status`; `stream:false` returns the running turn; a gated turn
ends `done` with `output: null` and the ask in `state.required_actions`; a
turn's `usage` is its last model call only — `tf.usage_total()` sums across
threads.

The benchmark itself lives in `bench/`: `bench/run.py` (the matrix, `just
bench`) and `bench/report.py` (the scorer, `just report`) — see `bench/README.md`.
