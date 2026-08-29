#!/usr/bin/env bash
# S3: redis pressure. Runs the scenario timeline against a running stack.
#
#   steady state -> a 66 MB blob lands in the shared redis (no change event) -> observe
#
# redis runs with maxmemory 64 MB and policy `noeviction` (payments'
# idempotency records must never vanish silently). Another tenant's bulk
# import -- one SETRANGE -- takes the store past its limit; from then on
# every write is refused with OOM. payments fails CLOSED on a cache-write
# failure (503 "payment store unavailable"), logging the same template as
# its steady-state cache hiccup (`cache write failed: <*>`), now at ERROR
# and at ~10/s instead of one every ~5 s, plus a `redis memory pressure`
# WARN with the store's own numbers. There is no deploy, no rollback target,
# and no new template at the culprit: the evidence is a burst of a known
# template and the store's memory numbers.
#
# The fault is left ACTIVE at exit. SCENARIO_FAST=1 (or S1_FAST=1) shortens the waits.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
DEPLOYER="${DEPLOYER:-./target/release/deployer}"
DATA_DIR="data/deploy"
FILL_BYTES="${S3_FILL_BYTES:-66000000}"

# "Known-but-rare" needs history: the engine's novelty baseline is the time
# BEFORE its default 5-minute incident window, and burst scoring needs at
# least 60 s of it. A steady state shorter than ~6 minutes leaves no
# baseline inside the run and the burst is undetermined -- so this
# scenario's steady state is long, on both timelines, by design.
STEADY="${S3_STEADY_SECS:-360}"
POST="${S3_POST_SECS:-90}"
FAST="${SCENARIO_FAST:-${S1_FAST:-0}}"
if [ "$FAST" = "1" ]; then STEADY=300; POST=70; fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="data/scenarios/s3/$RUN_ID"
mkdir -p "$RUN_DIR"
ts() { date -u +%Y-%m-%dT%H:%M:%S.%3NZ; }
say() { printf '[s3 %s] %s\n' "$(date -u +%H:%M:%S)" "$*"; }
rcli() { docker compose exec -T redis redis-cli "$@"; }

curl -sf "http://127.0.0.1:${GATEWAY_PORT:-8080}/health" >/dev/null || { echo "gateway not healthy; run 'just up' first" >&2; exit 1; }
"$DEPLOYER" --data-dir "$DATA_DIR" init --reset >/dev/null
rm -f data/knobs/*.json 2>/dev/null || true
rcli DEL spyglass:filler >/dev/null
say "clean state: all services v1; redis $(rcli INFO memory | tr -d '\r' | grep -E '^used_memory:' ) of $(rcli CONFIG GET maxmemory | tail -1) bytes, policy $(rcli CONFIG GET maxmemory-policy | tail -1). steady state for ${STEADY}s"
T_START="$(ts)"
sleep "$STEADY"

say "FAULT: another tenant's bulk import -- SETRANGE spyglass:filler ${FILL_BYTES} (no change event)"
rcli SETRANGE spyglass:filler "$FILL_BYTES" x >/dev/null
T_FAULT="$(ts)"
say "  redis now $(rcli INFO memory | tr -d '\r' | grep -E '^used_memory:') > maxmemory; writes refused (noeviction)"
say "observing for ${POST}s"
sleep "$POST"
T_END="$(ts)"

mkdir -p "$RUN_DIR/logs"
cp data/logs/*.jsonl "$RUN_DIR/logs/"
cp "$DATA_DIR/journal.jsonl" "$RUN_DIR/journal.jsonl"
cp "$DATA_DIR/current.json" "$RUN_DIR/current.json"
USED="$(rcli INFO memory | tr -d '\r' | grep -E '^used_memory:' | cut -d: -f2)"
MAXM="$(rcli CONFIG GET maxmemory | tail -1 | tr -d '\r')"
python3 - "$RUN_DIR" "$T_START" "$T_FAULT" "$T_END" "$STEADY" "$POST" "$FILL_BYTES" "$USED" "$MAXM" "$FAST" <<'PY'
import json, os, sys
d, t_start, t_fault, t_end, steady, post, fill, used, maxm, fast = sys.argv[1:]
m = {"scenario": "s3-redis-pressure", "run_id": os.path.basename(d), "fast": fast == "1",
     "seed": int(os.environ.get("LOADGEN_SEED", "42")), "rate": float(os.environ.get("LOADGEN_RATE", "10")),
     "t_start": t_start, "t_fault": t_fault, "t_end": t_end,
     "steady_secs": int(steady), "lead_secs": 0, "post_secs": int(post),
     "fault": {"kind": "redis_fill", "bytes": int(fill), "redis_used_memory_after": int(used), "redis_maxmemory": int(maxm),
               "policy": "noeviction", "change_event": None,
               "mechanism": "store over maxmemory -> every write refused (OOM) -> payments fails closed on its idempotency write (503)"}}
json.dump(m, open(os.path.join(d, "manifest.json"), "w"), indent=2)
PY
say "done. fault ACTIVE (redis over maxmemory). run dir: $RUN_DIR"
echo "$RUN_DIR"
