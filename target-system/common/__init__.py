"""Shared plumbing for the Spyglass target system.

The target is scenery, not the show -- but the evidence it emits is the raw
material for everything upstream, so its *shape* matters (README, C1):

  * one JSON object per log line: ts, service, level, msg, plus req_id, route,
    status, latency_ms, stack, deploy_id when known
  * Prometheus text metrics: requests_total, errors_total, latency_ms_bucket
  * logs go to stdout AND /var/log/spyglass/<instance>.jsonl, so the engine
    tails a plain file instead of docker's root-owned json logs
  * request ids propagate via x-request-id, so one checkout can be stitched
    across gateway -> orders -> payments without a tracing backend
"""
from __future__ import annotations

import hashlib
import json
import logging
import os
import sys
import time
import traceback
import uuid
from contextvars import ContextVar
from datetime import datetime, timezone
from pathlib import Path

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, PlainTextResponse
from prometheus_client import CONTENT_TYPE_LATEST, Counter, Histogram, generate_latest

SERVICE = os.environ.get("SERVICE_NAME", "unknown")
INSTANCE = os.environ.get("INSTANCE_NAME", SERVICE)
VERSION = os.environ.get("SERVICE_VERSION", "v1")
LOG_DIR = Path(os.environ.get("SPYGLASS_LOG_DIR", "/var/log/spyglass"))
DEPLOY_STATE = Path(os.environ.get("SPYGLASS_DEPLOY_STATE", "/deploy/current.json"))
KNOB_DIR = Path(os.environ.get("SPYGLASS_KNOB_DIR", "/knobs"))
UNLOGGED_PATHS = {"/health", "/metrics"}

req_id_var: ContextVar[str] = ContextVar("req_id", default="-")


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


# ---------------------------------------------------------------- logging
_EXTRA_KEYS = ("route", "status", "latency_ms", "deploy_id", "kind", "upstream",
               "upstream_version", "method", "path", "headers", "body", "replay", "detail")


class JsonFormatter(logging.Formatter):
    def format(self, record: logging.LogRecord) -> str:
        rec = {
            "ts": now_iso(), "service": SERVICE, "instance": INSTANCE, "version": VERSION,
            "level": record.levelname,
            "req_id": getattr(record, "req_id", None) or req_id_var.get(),
            "msg": record.getMessage(),
        }
        for k in _EXTRA_KEYS:
            v = getattr(record, k, None)
            if v is not None:
                rec[k] = v
        if record.exc_info:
            rec["stack"] = "".join(traceback.format_exception(*record.exc_info))[-2000:]
        return json.dumps(rec, separators=(",", ":"))


def _setup_logging() -> logging.Logger:
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    for h in list(root.handlers):
        root.removeHandler(h)
    fmt = JsonFormatter()
    sh = logging.StreamHandler(sys.stdout)
    sh.setFormatter(fmt)
    root.addHandler(sh)
    try:
        LOG_DIR.mkdir(parents=True, exist_ok=True)
        fh = logging.FileHandler(LOG_DIR / f"{INSTANCE}.jsonl")
        fh.setFormatter(fmt)
        root.addHandler(fh)
    except OSError as e:  # never crash on logging setup; stdout still works
        root.warning("log file unavailable, stdout only: %s", e)
    # uvicorn's own loggers flow through the JSON handlers; its access log is
    # replaced by our middleware, which knows about req_id and deploy_id.
    for name in ("uvicorn", "uvicorn.error", "uvicorn.access"):
        lg = logging.getLogger(name)
        lg.handlers.clear()
        lg.propagate = True
    logging.getLogger("uvicorn.access").setLevel(logging.WARNING)
    # httpx logs every outbound call at INFO -- a second copy of what our own
    # middleware already records. Library chatter is not system evidence.
    for name in ("httpx", "httpcore"):
        logging.getLogger(name).setLevel(logging.WARNING)
    return logging.getLogger(SERVICE)


log = _setup_logging()

# ---------------------------------------------------------------- metrics
REQUESTS = Counter("requests", "Requests handled", ["service", "route", "status"])
ERRORS = Counter("errors", "Responses with status >= 500", ["service", "route"])
LATENCY = Histogram("latency_ms", "Request latency in milliseconds", ["service", "route"],
                    buckets=(5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000))
UPSTREAM = Counter("upstream_requests", "Calls made to upstream services",
                   ["service", "upstream", "version", "status"])


# ---------------------------------------------------------------- noise
def noise_roll(key: str, salt: str) -> float:
    """Deterministic pseudo-random draw in [0, 1) from a request id.

    Background noise (WARN chatter, simulated latency) is a pure function of
    the request stream, so a pinned loadgen seed pins the noise too.
    """
    h = hashlib.sha256(f"{salt}:{key}".encode()).digest()
    return int.from_bytes(h[:8], "big") / 2**64


# ---------------------------------------------------------------- deploy state
_state_cache: dict = {"mtime": None, "data": {}}
_DEFAULT_DEPLOY = {"version": "v1", "deploy_id": None}


def current_deploy(service: str) -> dict:
    """Which version a service is routed to, per the deployer's current.json.

    Read per request but cached on mtime; the deployer writes the file with
    write-then-rename, so we never observe a torn state.
    """
    try:
        st = DEPLOY_STATE.stat()
    except FileNotFoundError:
        return dict(_DEFAULT_DEPLOY)
    if st.st_mtime_ns != _state_cache["mtime"]:
        try:
            _state_cache["data"] = json.loads(DEPLOY_STATE.read_text())
            _state_cache["mtime"] = st.st_mtime_ns
        except (OSError, json.JSONDecodeError):
            return dict(_DEFAULT_DEPLOY)
    entry = _state_cache["data"].get(service) or {}
    return {"version": entry.get("version", "v1"), "deploy_id": entry.get("deploy_id")}


_knob_cache: dict = {}


def knob(name: str) -> dict:
    """Scenario knobs: /knobs/<name>.json, read per request, cached on mtime.

    Knobs are how a scenario changes the *environment* without a change
    event -- the gateway's latency blip (S2's decoy) and the fraud vendor's
    degradation (S6). They are not telemetry: no tool in either benchmark
    condition reads this directory, and the services never log them. An
    absent or malformed file means "no knob".
    """
    path = KNOB_DIR / f"{name}.json"
    try:
        st = path.stat()
    except FileNotFoundError:
        _knob_cache.pop(name, None)
        return {}
    c = _knob_cache.get(name)
    if c is None or c[0] != st.st_mtime_ns:
        try:
            c = (st.st_mtime_ns, json.loads(path.read_text()))
        except (OSError, json.JSONDecodeError):
            c = (st.st_mtime_ns, {})
        _knob_cache[name] = c
    return c[1] if isinstance(c[1], dict) else {}


# ---------------------------------------------------------------- app wiring
def install(app: FastAPI) -> None:
    """Health, metrics, and the observe middleware. Call once per service."""

    @app.get("/health")
    async def health():
        return {"ok": True, "service": SERVICE, "instance": INSTANCE, "version": VERSION}

    @app.get("/metrics")
    async def metrics():
        return PlainTextResponse(generate_latest(), media_type=CONTENT_TYPE_LATEST)

    @app.middleware("http")
    async def observe(request: Request, call_next):
        path = request.url.path
        if path in UNLOGGED_PATHS:
            return await call_next(request)
        rid = request.headers.get("x-request-id") or str(uuid.uuid4())
        # Synthetic replay traffic (the evidence engine's causal check) says
        # so on its request line, so a reader of the raw log can tell an
        # experiment from a customer. The engine also keys off the req_id.
        replay = request.headers.get("x-spyglass-replay")
        token = req_id_var.set(rid)
        own = current_deploy(SERVICE)["deploy_id"]
        t0 = time.perf_counter()
        try:
            response = await call_next(request)
        except Exception as exc:
            # An unhandled exception is the most valuable log line in the
            # system: ERROR level, the exception's own message, a stack.
            ms = round((time.perf_counter() - t0) * 1000, 1)
            log.error(str(exc) or exc.__class__.__name__, exc_info=True,
                      extra={"route": path, "status": 500, "latency_ms": ms, "deploy_id": own, "replay": replay})
            REQUESTS.labels(SERVICE, path, "500").inc()
            ERRORS.labels(SERVICE, path).inc()
            LATENCY.labels(SERVICE, path).observe(ms)
            req_id_var.reset(token)
            return JSONResponse({"error": exc.__class__.__name__, "req_id": rid},
                                status_code=500, headers={"x-request-id": rid})
        ms = round((time.perf_counter() - t0) * 1000, 1)
        status = response.status_code
        REQUESTS.labels(SERVICE, path, str(status)).inc()
        LATENCY.labels(SERVICE, path).observe(ms)
        if status >= 500:
            ERRORS.labels(SERVICE, path).inc()
            log.error("request failed", extra={"route": path, "status": status, "latency_ms": ms, "deploy_id": own, "replay": replay})
        else:
            log.info("request completed", extra={"route": path, "status": status, "latency_ms": ms, "deploy_id": own, "replay": replay})
        response.headers["x-request-id"] = rid
        req_id_var.reset(token)
        return response


def run(app: FastAPI, default_port: int) -> None:
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=int(os.environ.get("PORT", default_port)),
                log_config=None, access_log=False)
