#!/usr/bin/env python3
"""Terminal error-rate dashboard + threshold alert for the target system.

Polls the gateway's /metrics every few seconds, prints the 5xx share of
/checkout as a bar, shows which payments version orders is routed to, and
fires an alert when the rate stays above threshold for two consecutive
windows. The alert is written to data/alerts/latest.json AND opens a TrueForge
session with the alert as its first message -- the spec's data-flow step 3:

    "payments error alert firing -- investigate; roll back if a deploy
     caused it."

Which agent answers is config: SPYGLASS_AGENT names a saved TrueForge agent
(Phase 3 registers the Spyglass SOP under that name); unset, a bare inline
agent on MODEL_A is used so the session still opens. --no-session disables.
This is the `just watch` from ADR-013.
"""
import json
import os
import re
import argparse
import functools
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import tf  # noqa: E402  (scripts/tf.py, the REST driver)

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


def _dotenv(key: str, default: str = "") -> str:
    if os.environ.get(key):
        return os.environ[key]
    try:
        for line in Path(".env").read_text().splitlines():
            if line.startswith(f"{key}="):
                return line.split("=", 1)[1].strip()
    except OSError:
        pass
    return default


ALERT_MESSAGE = ("payments error alert firing -- gateway /checkout 5xx rate {rate:.1%} "
                 "(threshold {thr:.0%}) for 2 consecutive {iv:.0f}s windows. "
                 "Investigate; roll back if a deploy caused it.")


def resolve_model() -> str:
    """MODEL_A from .env, matched against what TrueForge actually serves.
    The catalog exposes gemini-3.6-flash as google-gemini/gemini-3-6-flash;
    match on model_id first, then on the dashed name, then take the first."""
    want = _dotenv("MODEL_A")
    models = tf._req("GET", "/models")["data"]
    for m in models:
        if m.get("model_id") == want or m.get("name", "").split("/")[-1] == want.replace(".", "-"):
            return m["name"]
    return models[0]["name"]


def open_session(alert: dict) -> dict | None:
    """Open a TrueForge session and post the alert as its first turn.
    Non-blocking: the turn runs in the harness; we return the ids."""
    agent_name = _dotenv("SPYGLASS_AGENT")
    if agent_name:
        spec = {"agent": {"name": agent_name}}
    else:
        spec = {"agent": {"spec": {
            "model": {"name": resolve_model()},
            "instructions": ("You are an on-call incident responder. No investigation tools are "
                             "attached to this session yet; acknowledge the alert, state what "
                             "evidence you would need, and wait."),
        }}}
    sid = tf._req("POST", "/sessions", spec)["data"]["id"]
    msg = ALERT_MESSAGE.format(rate=alert["observed_rate"], thr=alert["threshold"], iv=INTERVAL)
    turn = tf._req("POST", f"/sessions/{sid}/turns",
                   {"input": [tf.user_message(msg)], "stream": False})["data"]
    return {"session_id": sid, "turn_id": turn["id"], "agent": agent_name or "inline",
            "url": f"{tf.BASE}/sessions/{sid}"}


def routing():
    try:
        p = json.loads(STATE.read_text()).get("payments", {})
        return f"{p.get('version','?')} ({p.get('deploy_id') or 'initial'})"
    except (OSError, json.JSONDecodeError):
        return "?"


def main(open_sessions: bool):
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
                if open_sessions:
                    try:
                        alert["session"] = open_session(alert)
                    except Exception as e:  # the alert must land even if the harness is down
                        alert["session_error"] = f"{e.__class__.__name__}: {str(e)[:200]}"
                (ALERTS / "latest.json").write_text(json.dumps(alert, indent=2))
                print(f"\n  *** ALERT *** {alert['message']}\n  -> {ALERTS/'latest.json'}")
                if "session" in alert:
                    print(f"  -> TrueForge session opened: {alert['session']['session_id']} "
                          f"(agent={alert['session']['agent']})  {alert['session']['url']}")
                elif "session_error" in alert:
                    print(f"  -> could not open TrueForge session: {alert['session_error']}")
                print()
            if rate <= THRESHOLD and alerted and hot == 0:
                alerted = False
                print("  (recovered: rate back under threshold)")
        prev = cur
        time.sleep(INTERVAL)


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--no-session", action="store_true",
                    help="announce only; do not open a TrueForge session on alert")
    args = ap.parse_args()
    try:
        main(open_sessions=not args.no_session)
    except KeyboardInterrupt:
        sys.exit(0)
