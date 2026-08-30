#!/usr/bin/env bash
# S1: payment regression. Runs the scenario timeline against a running stack.
#
#   steady state -> benign orders deploy (D-1) -> lead -> payments v2 (D-2) -> observe
#
# All wall-clock timestamps go to the run manifest; ground-truth.yaml stays
# relative. S1_FAST=1 compresses the waits for development and CI; the default
# timeline is the one the demo uses. The fault is left ACTIVE at exit -- the
# agent's job is to find and undo it.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
DEPLOYER="${DEPLOYER:-./target/release/deployer}"
DATA_DIR="data/deploy"

STEADY="${S1_STEADY_SECS:-120}"
LEAD="${S1_BENIGN_LEAD_SECS:-360}"
POST="${S1_POST_SECS:-90}"
FAST="${SCENARIO_FAST:-${S1_FAST:-0}}"
if [ "$FAST" = "1" ]; then STEADY=40; LEAD=50; POST=70; fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="data/scenarios/s1/$RUN_ID"
mkdir -p "$RUN_DIR"
ts() { date -u +%Y-%m-%dT%H:%M:%S.%3NZ; }
say() { printf '[s1 %s] %s\n' "$(date -u +%H:%M:%S)" "$*"; }

curl -sf "http://127.0.0.1:${GATEWAY_PORT:-8080}/health" >/dev/null || { echo "gateway not healthy; run 'just up' first" >&2; exit 1; }
"$DEPLOYER" --data-dir "$DATA_DIR" init --reset >/dev/null
rm -f data/knobs/*.json 2>/dev/null || true
say "clean state: all services v1. steady state for ${STEADY}s"
T_START="$(ts)"
sleep "$STEADY"

say "benign deploy: orders v1.1 (decoy)"
BENIGN="$("$DEPLOYER" --data-dir "$DATA_DIR" deploy orders v1.1 --actor ci-bot)"
T_BENIGN="$(ts)"
say "  $BENIGN"
say "lead time ${LEAD}s"
sleep "$LEAD"

say "FAULT: deploy payments v2"
FAULT="$("$DEPLOYER" --data-dir "$DATA_DIR" deploy payments v2 --actor deploy-bot)"
T_FAULT="$(ts)"
say "  $FAULT"
say "observing for ${POST}s"
sleep "$POST"
T_END="$(ts)"

mkdir -p "$RUN_DIR/logs"
cp data/logs/*.jsonl "$RUN_DIR/logs/"
cp "$DATA_DIR/journal.jsonl" "$RUN_DIR/journal.jsonl"
cp "$DATA_DIR/current.json" "$RUN_DIR/current.json"
python3 - "$RUN_DIR" "$T_START" "$T_BENIGN" "$T_FAULT" "$T_END" "$STEADY" "$LEAD" "$POST" "$BENIGN" "$FAULT" <<'PY'
import json, os, sys
d, t_start, t_benign, t_fault, t_end, steady, lead, post, benign, fault = sys.argv[1:]
m = {"scenario": "s1-payment-regression", "run_id": os.path.basename(d),
     "fast": os.environ.get("SCENARIO_FAST", os.environ.get("S1_FAST", "0")) == "1",
     "fault": {"kind": "deploy", "service": "payments", "version": "v2"},
     "seed": int(os.environ.get("LOADGEN_SEED", "42")), "rate": float(os.environ.get("LOADGEN_RATE", "10")),
     "t_start": t_start, "t_benign_deploy": t_benign, "t_fault": t_fault, "t_end": t_end,
     "steady_secs": int(steady), "lead_secs": int(lead), "post_secs": int(post),
     "benign_deploy": json.loads(benign), "fault_deploy": json.loads(fault)}
json.dump(m, open(os.path.join(d, "manifest.json"), "w"), indent=2)
PY
say "done. fault ACTIVE (payments -> v2). run dir: $RUN_DIR"
echo "$RUN_DIR"
