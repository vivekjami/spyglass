#!/usr/bin/env bash
# S6: insufficient evidence. Runs the scenario timeline against a running stack.
#
#   steady state -> benign orders deploy (D-1, normal life) -> lead -> the fraud vendor degrades (knob) -> observe
#
# orders calls the fraudcheck vendor synchronously before every charge,
# fails open on timeout (5 s), and never logs the call: the dependency is
# in the topology and absent from the telemetry. When the vendor slows to
# 9 s on 12% of calls, orders' latency rises on those requests and nothing
# else moves -- no error, no deploy, no new template, no metric at the
# cause. The correct investigation ends in a calibrated refusal: no action,
# and a statement of what evidence would decide it.
#
# The fault is left ACTIVE at exit. SCENARIO_FAST=1 (or S1_FAST=1) shortens the waits.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
DEPLOYER="${DEPLOYER:-./target/release/deployer}"
DATA_DIR="data/deploy"
KNOBS="data/knobs"
SHARE="${S6_DEGRADE_SHARE:-0.12}"
LAT_MS="${S6_DEGRADE_MS:-9000}"

STEADY="${S6_STEADY_SECS:-120}"
LEAD="${S6_BENIGN_LEAD_SECS:-360}"
POST="${S6_POST_SECS:-90}"
FAST="${SCENARIO_FAST:-${S1_FAST:-0}}"
# The fast lead stays > 120 s: the engine joins a deploy to a changepoint
# within +-120 s, and the benign deploy must be OUTSIDE that window, or the
# fast timeline would be a different scenario from the pre-registered one.
if [ "$FAST" = "1" ]; then STEADY=40; LEAD=130; POST=70; fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="data/scenarios/s6/$RUN_ID"
mkdir -p "$RUN_DIR" "$KNOBS"
ts() { date -u +%Y-%m-%dT%H:%M:%S.%3NZ; }
say() { printf '[s6 %s] %s\n' "$(date -u +%H:%M:%S)" "$*"; }

curl -sf "http://127.0.0.1:${GATEWAY_PORT:-8080}/health" >/dev/null || { echo "gateway not healthy; run 'just up' first" >&2; exit 1; }
"$DEPLOYER" --data-dir "$DATA_DIR" init --reset >/dev/null
rm -f "$KNOBS/gateway.json" "$KNOBS/fraudcheck.json"
say "clean state: all services v1, no knobs. steady state for ${STEADY}s"
T_START="$(ts)"
sleep "$STEADY"

say "benign deploy: orders v1.1 (normal life)"
BENIGN="$("$DEPLOYER" --data-dir "$DATA_DIR" deploy orders v1.1 --actor ci-bot)"
T_BENIGN="$(ts)"
say "  $BENIGN"
say "lead time ${LEAD}s"
sleep "$LEAD"

say "FAULT: the fraud vendor degrades -- ${LAT_MS} ms on ${SHARE} of calls (no change event, unobserved dependency)"
printf '{"degrade": {"share": %s, "latency_ms": %s}}\n' "$SHARE" "$LAT_MS" > "$KNOBS/fraudcheck.json"
T_FAULT="$(ts)"
say "observing for ${POST}s"
sleep "$POST"
T_END="$(ts)"

mkdir -p "$RUN_DIR/logs"
cp data/logs/*.jsonl "$RUN_DIR/logs/"
cp "$DATA_DIR/journal.jsonl" "$RUN_DIR/journal.jsonl"
cp "$DATA_DIR/current.json" "$RUN_DIR/current.json"
python3 - "$RUN_DIR" "$T_START" "$T_BENIGN" "$T_FAULT" "$T_END" "$STEADY" "$LEAD" "$POST" "$BENIGN" "$SHARE" "$LAT_MS" "$FAST" <<'PY'
import json, os, sys
d, t_start, t_benign, t_fault, t_end, steady, lead, post, benign, share, lat, fast = sys.argv[1:]
m = {"scenario": "s6-insufficient-evidence", "run_id": os.path.basename(d), "fast": fast == "1",
     "seed": int(os.environ.get("LOADGEN_SEED", "42")), "rate": float(os.environ.get("LOADGEN_RATE", "10")),
     "t_start": t_start, "t_benign_deploy": t_benign, "t_fault": t_fault, "t_end": t_end,
     "steady_secs": int(steady), "lead_secs": int(lead), "post_secs": int(post),
     "benign_deploy": json.loads(benign),
     "fault": {"kind": "dependency_degradation", "dependency": "fraudcheck", "observed": False, "change_event": None,
               "share": float(share), "latency_ms": int(lat),
               "mechanism": "vendor slow on a share of calls; orders fails open after its 5 s timeout and logs nothing about the call"}}
json.dump(m, open(os.path.join(d, "manifest.json"), "w"), indent=2)
PY
say "done. fault ACTIVE (fraudcheck degraded via knob). run dir: $RUN_DIR"
echo "$RUN_DIR"
