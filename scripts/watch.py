#!/usr/bin/env python3
"""Terminal error-rate dashboard + threshold alert for the target system.

Polls the gateway's /metrics every few seconds, prints the 5xx share of
/checkout as a bar, shows which payments version orders is routed to, and
fires an alert when the rate stays above threshold for two consecutive
windows. The alert is written to data/alerts/latest.json; Phase 3 turns that
into "open a TrueForge session". This is the `just watch` from ADR-013.
"""
import json
import os
import re
import functools
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

def _port() -> str:
    """GATEWAY_PORT from the environment, else from .env (where `just` and
    docker compose read it), else 8080. Running this script directly must
    watch the same gateway `just up` published."""
    if os.environ.get("GATEWAY_PORT"):
        return os.environ["GATEWAY_PORT"]
    try:
        for line in Path(".env").read_text().splitlines():
            if line.startswith("GATEWAY_PORT="):
                return line.split("=", 1)[1].strip()
    except OSError:
        pass
    return "8080"


METRICS = f"http://127.0.0.1:{_port()}/metrics"
STATE = Path("data/deploy/current.json")
ALERTS = Path("data/alerts")
INTERVAL = 5.0
THRESHOLD = 0.05
print = functools.partial(print, flush=True)  # noqa: A001 -- never block-buffer a dashboard
LINE = re.compile(r'^requests_total\{([^}]*)\}\s+([0-9.e+]+)$', re.M)


def scrape():
    txt = urllib.request.urlopen(METRICS, timeout=3).read().decode()
    tot = err = 0.0
    for labels, val in LINE.findall(txt):
        if 'route="/checkout"' not in labels:
            continue
        v = float(val)
        tot += v
        if 'status="5' in labels:
            err += v
    return tot, err


def routing():
    try:
        p = json.loads(STATE.read_text()).get("payments", {})
        return f"{p.get('version','?')} ({p.get('deploy_id') or 'initial'})"
    except (OSError, json.JSONDecodeError):
        return "?"


def main():
    prev = None
    hot = 0
    alerted = False
    print(f"watching {METRICS} every {INTERVAL:.0f}s; alert when 5xx > {THRESHOLD:.0%} for 2 windows")
    while True:
        try:
            cur = scrape()
        except Exception as e:
            print(f"{datetime.now():%H:%M:%S}  scrape failed: {e.__class__.__name__}")
            time.sleep(INTERVAL)
            continue
        if prev is not None:
            dt, de = cur[0] - prev[0], cur[1] - prev[1]
            rate = de / dt if dt else 0.0
            bar = "█" * int(rate * 40)
            print(f"{datetime.now():%H:%M:%S}  {dt/INTERVAL:5.1f} req/s  5xx {100*rate:5.1f}%  "
                  f"payments={routing():<16} {bar}")
            hot = hot + 1 if rate > THRESHOLD else 0
            if hot >= 2 and not alerted:
                alerted = True
                ALERTS.mkdir(parents=True, exist_ok=True)
                alert = {"ts": datetime.now(timezone.utc).isoformat(timespec="seconds"),
                         "alert": "gateway_checkout_5xx_rate",
                         "message": f"payments error alert firing: gateway /checkout 5xx rate {rate:.1%} "
                                    f"(threshold {THRESHOLD:.0%}) for 2 consecutive {INTERVAL:.0f}s windows",
                         "observed_rate": round(rate, 4), "threshold": THRESHOLD}
                (ALERTS / "latest.json").write_text(json.dumps(alert, indent=2))
                print(f"\n  *** ALERT *** {alert['message']}\n  -> {ALERTS/'latest.json'}\n")
            if rate <= THRESHOLD and alerted and hot == 0:
                alerted = False
                print("  (recovered: rate back under threshold)")
        prev = cur
        time.sleep(INTERVAL)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(0)
