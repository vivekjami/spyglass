#!/usr/bin/env bash
# S2: timeout cascade. Runs the scenario timeline against a running stack.
#
#   steady state -> gateway latency blip (decoy, knob) -> quiet -> orders v1.2 (D-1, config-only) -> observe
#
# orders v1.2 is a config release: the fraud client moves to the vendor's v2
# API (synchronous scoring, ~1.5 s; ~9 s "deep scoring" for premium/corporate
# cards) and its timeout doubles from 5 s to 10 s. With the gateway's own
# 8 s upstream timeout, every deep-scored order now times out at the edge:
# latency rises at orders, then at the gateway, then the 5xx follow. No
# service raises, no stack trace, no new template at the culprit -- the
# cause is a change event plus a latency cascade.
#
# The fault is left ACTIVE at exit. SCENARIO_FAST=1 (or S1_FAST=1) shortens
# the waits; the quiet gap after the blip stays > 120 s so the blip's
# changepoints cannot be deploy-correlated with the fault by the engine.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
DEPLOYER="${DEPLOYER:-./target/release/deployer}"
DATA_DIR="data/deploy"
KNOBS="data/knobs"

STEADY="${S2_STEADY_SECS:-120}"
BLIP="${S2_BLIP_SECS:-30}"
QUIET="${S2_QUIET_SECS:-180}"
POST="${S2_POST_SECS:-90}"
FAST="${SCENARIO_FAST:-${S1_FAST:-0}}"
if [ "$FAST" = "1" ]; then STEADY=90; BLIP=30; QUIET=125; POST=70; fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="data/scenarios/s2/$RUN_ID"
mkdir -p "$RUN_DIR" "$KNOBS"
ts() { date -u +%Y-%m-%dT%H:%M:%S.%3NZ; }
say() { printf '[s2 %s] %s\n' "$(date -u +%H:%M:%S)" "$*"; }

curl -sf "http://127.0.0.1:${GATEWAY_PORT:-8080}/health" >/dev/null || { echo "gateway not healthy; run 'just up' first" >&2; exit 1; }
"$DEPLOYER" --data-dir "$DATA_DIR" init --reset >/dev/null
rm -f "$KNOBS/gateway.json" "$KNOBS/fraudcheck.json"
say "clean state: all services v1, no knobs. steady state for ${STEADY}s"
T_START="$(ts)"
sleep "$STEADY"

say "decoy: gateway latency blip (+400 ms on 50% of requests) for ${BLIP}s -- nobody's change"
echo '{"blip_ms": 400, "blip_share": 0.5}' > "$KNOBS/gateway.json"
T_BLIP_START="$(ts)"
sleep "$BLIP"
rm -f "$KNOBS/gateway.json"
T_BLIP_END="$(ts)"
say "quiet for ${QUIET}s"
sleep "$QUIET"

say "FAULT: deploy orders v1.2 (config-only: fraud client -> vendor API v2, timeout 5 s -> 10 s)"
FAULT="$("$DEPLOYER" --data-dir "$DATA_DIR" deploy orders v1.2 --actor config-bot)"
T_FAULT="$(ts)"
say "  $FAULT"
say "observing for ${POST}s"
sleep "$POST"
T_END="$(ts)"

mkdir -p "$RUN_DIR/logs"
cp data/logs/*.jsonl "$RUN_DIR/logs/"
cp "$DATA_DIR/journal.jsonl" "$RUN_DIR/journal.jsonl"
cp "$DATA_DIR/current.json" "$RUN_DIR/current.json"
python3 - "$RUN_DIR" "$T_START" "$T_BLIP_START" "$T_BLIP_END" "$T_FAULT" "$T_END" "$STEADY" "$BLIP" "$QUIET" "$POST" "$FAULT" "$FAST" <<'PY'
import json, os, sys
d, t_start, t_b0, t_b1, t_fault, t_end, steady, blip, quiet, post, fault, fast = sys.argv[1:]
m = {"scenario": "s2-timeout-cascade", "run_id": os.path.basename(d), "fast": fast == "1",
     "seed": int(os.environ.get("LOADGEN_SEED", "42")), "rate": float(os.environ.get("LOADGEN_RATE", "10")),
     "t_start": t_start, "t_blip_start": t_b0, "t_blip_end": t_b1, "t_fault": t_fault, "t_end": t_end,
     "steady_secs": int(steady), "blip_secs": int(blip), "quiet_secs": int(quiet), "lead_secs": int(blip) + int(quiet), "post_secs": int(post),
     "decoy": {"kind": "latency_blip", "service": "gateway", "blip_ms": 400, "blip_share": 0.5, "from": t_b0, "to": t_b1},
     "fault": {"kind": "deploy", "service": "orders", "version": "v1.2", "config_only": True,
               "mechanism": "fraud client -> vendor API v2 (1.5 s; 9 s deep scoring for premium/corporate), timeout 5 s -> 10 s; gateway upstream timeout is 8 s"},
     "fault_deploy": json.loads(fault)}
json.dump(m, open(os.path.join(d, "manifest.json"), "w"), indent=2)
PY
say "done. fault ACTIVE (orders -> v1.2). run dir: $RUN_DIR"
echo "$RUN_DIR"
