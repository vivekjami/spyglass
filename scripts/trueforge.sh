#!/usr/bin/env bash
# Start / stop / status the TrueForge harness with Spyglass's environment.
#
#   scripts/trueforge.sh start    launch in the background, wait until healthy
#   scripts/trueforge.sh stop     stop it (matches by listening port, not name)
#   scripts/trueforge.sh status   report health + sandbox availability
#   scripts/trueforge.sh logs     tail the log
#
# Note: do NOT `pkill -f trueforge` — the pattern matches any shell whose
# command line mentions trueforge, including the one running this script.
# We resolve the PID from whoever holds the port instead.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/scripts/env.sh" >/dev/null

LOG="$ROOT/.local/trueforge.log"
PORT="${TRUEFORGE_PORT:-8790}"

port_pid() { ss -tlnpH "sport = :$PORT" 2>/dev/null | grep -oP 'pid=\K[0-9]+' | head -1; }

case "${1:-status}" in
  start)
    if [ -n "$(port_pid)" ]; then echo "already running on :$PORT (pid $(port_pid))"; exit 0; fi
    mkdir -p "$(dirname "$LOG")"
    [ -f "$LOG" ] && mv "$LOG" "$LOG.$(date +%s)"
    nohup npx -y @truefoundry/trueforge@latest >"$LOG" 2>&1 &
    echo "starting (log: $LOG) ..."
    for _ in $(seq 1 60); do
      if curl -sf -o /dev/null "$TRUEFORGE_URL/api/v1/capabilities"; then
        echo "healthy: $TRUEFORGE_URL (pid $(port_pid))"; exit 0
      fi
      sleep 2
    done
    echo "did not become healthy in 120s; last log lines:" >&2; tail -20 "$LOG" >&2; exit 1
    ;;
  stop)
    pid="$(port_pid)"
    if [ -z "$pid" ]; then echo "not running"; exit 0; fi
    kill "$pid" && echo "stopped pid $pid"
    ;;
  status)
    if [ -z "$(port_pid)" ]; then echo "not running"; exit 1; fi
    echo "running on :$PORT (pid $(port_pid))"
    curl -s "$TRUEFORGE_URL/api/v1/capabilities" | python3 -m json.tool
    ;;
  logs) tail -f "$LOG" ;;
  *) echo "usage: $0 {start|stop|status|logs}" >&2; exit 2 ;;
esac
