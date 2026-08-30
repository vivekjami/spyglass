#!/usr/bin/env bash
# Start / stop / status the MCP servers the agents talk to.
#
#   deployer  :8792  propose_rollback, rollback (approval-required), current_versions -- the write plane
#   rawtools  :8793  tail_logs, grep_logs, get_metric, list_services, deploy_events, http_request -- the baseline
#   engine    :8791  build_evidence_bundle, novel_templates, detect_changepoints, error_delta, deploy_events, search_logs,
#                    freshness_watermark, get_evidence, service_topology, get_exemplar_request, replay_exemplar, verify_recovery -- the evidence plane
#   ablation  :8794  the same engine binary and config with --ablation no-novelty (novel_templates off, w_n = 0,
#                    no template candidates in bundles) -- benchmark condition A1; own segment dir
#
# Liveness is by listening port, never by process-name grep: `pgrep -f` matches
# the shell running this script, which is how `just mcp-up` silently did
# nothing the first time.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
# shellcheck source=/dev/null
[ -f .env ] && set -a && source .env && set +a
mkdir -p .local data/deploy data/logs

declare -A CMD=(
  [engine]="./target/release/spyglass-mcp --config spyglass.toml --port 8791"
  [deployer]="./target/release/deployer --data-dir data/deploy serve --port 8792"
  [rawtools]="./target/release/rawtools-mcp --log-dir data/logs --deploy-dir data/deploy --port 8793"
  [ablation]="./target/release/spyglass-mcp --config spyglass.toml --port 8794 --ablation no-novelty"
)
declare -A PORT=([engine]=8791 [deployer]=8792 [rawtools]=8793 [ablation]=8794)
ALL="engine deployer rawtools ablation"

pid_on() { ss -tlnpH "sport = :$1" 2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1; }

start_one() {
  local name=$1
  if [ -n "$(pid_on "${PORT[$name]}")" ]; then echo "$name: already on :${PORT[$name]}"; return; fi
  nohup ${CMD[$name]} > ".local/$name-mcp.log" 2>&1 &
  for _ in $(seq 1 20); do [ -n "$(pid_on "${PORT[$name]}")" ] && break; sleep 0.25; done
  if [ -n "$(pid_on "${PORT[$name]}")" ]; then echo "$name: http://localhost:${PORT[$name]}/mcp (pid $(pid_on "${PORT[$name]}"))"
  else echo "$name: FAILED to start; see .local/$name-mcp.log" >&2; tail -5 ".local/$name-mcp.log" >&2; return 1; fi
}

case "${1:-status}" in
  start)  for n in $ALL; do start_one "$n" || exit 1; done ;;
  stop)   for n in $ALL; do p="$(pid_on "${PORT[$n]}")"; [ -n "$p" ] && kill "$p" && echo "$n: stopped ($p)" || echo "$n: not running"; done ;;
  restart) "$0" stop; sleep 0.5; "$0" start ;;
  status) for n in $ALL; do p="$(pid_on "${PORT[$n]}")"; [ -n "$p" ] && echo "$n: up on :${PORT[$n]} (pid $p)" || echo "$n: down"; done ;;
  *) echo "usage: $0 {start|stop|restart|status}" >&2; exit 2 ;;
esac
