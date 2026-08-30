# Spyglass task runner. `just` with no args lists recipes.
# Install without root: scripts/install-just.sh

set shell := ["bash", "-euo", "pipefail", "-c"]
set dotenv-load := true
export SPYGLASS_UID := `id -u`
export SPYGLASS_GID := `id -g`

deployer := "./target/release/deployer"

default:
    @just --list

# Build the Rust workspace and the target-system image.
build:
    cargo build --release
    docker compose build

# Bring the target system up (creates data dirs, initialises deploy state).
up: 
    mkdir -p data/logs data/deploy data/knobs
    {{deployer}} --data-dir data/deploy init >/dev/null
    docker compose up -d --wait
    @echo "target system up: gateway http://127.0.0.1:${GATEWAY_PORT:-8080}  orders :${ORDERS_PORT:-8081}  payments-v1 :${PAYMENTS_V1_PORT:-8082}  payments-v2 :${PAYMENTS_V2_PORT:-8083}"

down:
    docker compose down

# Stop everything and wipe runtime data (logs, deploy state, postgres). Scenario runs are kept.
clean:
    docker compose down -v --remove-orphans 2>/dev/null || true
    rm -rf data/logs data/deploy data/segments data/segments-no-novelty data/knobs

# Run a scenario from clean state: `just scenario s1` (s1 | s2 | s3 | s6). SCENARIO_FAST=1 (or S1_FAST=1) shortens the timeline.
# The evidence engines are restarted before the injection so their history is the scenario's alone.
scenario name:
    just clean
    just up
    scripts/mcp.sh restart
    bash scenarios/{{name}}-*/inject.sh

# Start the MCP servers the agents use: engine (evidence plane, :8791), deployer (rollback, :8792), rawtools (baseline, :8793), ablation engine (A1, :8794).
mcp-up:
    scripts/mcp.sh start

mcp-down:
    scripts/mcp.sh stop

# Register MCP servers + every bench/conditions/*.json as a named TrueForge agent (idempotent).
tf-setup:
    python3 scripts/tf-setup.py

# Run one instrumented investigation: `just investigate baseline` (add: --approval allow|deny|ask, --tag ...).
investigate condition *args:
    python3 scripts/investigate.py --condition {{condition}} {{args}}

# Live error-rate dashboard + threshold alert (the watcher).
watch:
    python3 scripts/watch.py

# Compare the last two S1 runs (or two named run dirs) against ground-truth tolerances.
s1-check *args:
    python3 scripts/s1-curve.py --compare {{args}}

# Scenario acceptance for any scenario: `just scenario-check s2` compares its last two runs (5xx curve, p95 latency) against ground truth.
scenario-check name *args:
    python3 scripts/scenario-curve.py --scenario {{name}} --compare {{args}}

# Phase 5 acceptance on the latest S1 run: fault changepoint ±10 s + D-2 annotated, decoy deploy not blamed, steady state clean.
s5-check *args:
    python3 scripts/changepoint-check.py {{args}}

# Phase 6+7 acceptance on the latest S1 run: top 3 = deploy + changepoint + template, w_n=0 reorders, <= 20 items / 8 kB, reduction_ratio.
s7-check *args:
    python3 scripts/bundle-check.py {{args}}

# Phase 8 acceptance on the latest S1 run: sanitized exemplar, replay v1 ~0/20 vs v2 ~19-20/20 (measured), ledger entries, negative control, no leakage into evidence.
s8-check *args:
    python3 scripts/replay-check.py {{args}}

# Phase 9 acceptance, live: double-fire = 1 rollback + 1 noop; approve-after-manual-rollback aborts; expired/restated proposals refused; engine closes only after 2 clean checks and escalates on worsening; budget backstop. Leaves payments at v1.
s9-check *args:
    python3 scripts/gate-check.py {{args}}

# Validate every ground-truth.yaml against scenarios/SCHEMA.md.
validate:
    python3 scripts/validate-ground-truth.py scenarios/*/ground-truth.yaml

logs svc:
    docker compose logs -f --no-log-prefix {{svc}}

# THE loop: fresh S1 incident -> Spyglass agent investigates -> causal replay -> gated rollback -> verified recovery -> ledger re-check.
# Approval is asked for interactively unless DEMO_APPROVAL=allow (unattended).
demo:
    just mcp-up
    just tf-setup
    S1_FAST=1 just scenario s1
    python3 scripts/investigate.py --condition spyglass --scenario s1 --approval ${DEMO_APPROVAL:-ask} --tag demo

# Re-execute every deterministic ledger entry against the live engine and compare digests: `just ledger-check ledger/<id>.jsonl`
ledger-check file:
    python3 scripts/ledger-check.py {{file}}

# THE benchmark: {baseline, spyglass, ablation-no-novelty} x {s1, s2, s3, s6} x 3 repeats, unattended (auto-approve), every run committed.
# `just bench` runs the whole matrix (~3 h); `just bench --scenarios s1 --conditions spyglass --repeats 1` runs one cell.
bench *args:
    python3 bench/run.py {{args}}

# Aggregate bench/results/*.json into the tables in docs/benchmark.md and README.md (never hand-edited).
report:
    python3 bench/report.py
