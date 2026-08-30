"""payments: charges a card. One codebase, two always-on instances.

  SERVICE_VERSION=v1  the known-good version
  SERVICE_VERSION=v2  carries scenario S1's seeded regression

Both run side by side in Compose so a failing request can be replayed against
each without touching live routing (README, Sandbox Causal Verification).
"""
from __future__ import annotations

import asyncio
import json
import os
import time
import uuid
from contextlib import asynccontextmanager

import redis.asyncio as redis
from redis.exceptions import RedisError
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

from common import VERSION, install, log, noise_roll, req_id_var, run

SUPPORTED = {"USD", "EUR", "GBP", "JPY", "CAD", "AUD"}
REDIS_URL = os.environ.get("REDIS_URL", "redis://redis:6379/0")
CACHE_HICCUP_RATE = float(os.environ.get("PAYMENTS_CACHE_HICCUP_RATE", "0.02"))
_pressure_warned_at = 0.0


class UnsupportedCurrency(Exception):
    """Raised by v2's validator. Deliberately unhandled: it must escape the
    handler as a 500 with a stack trace, because that is what a real
    regression looks like in the logs."""


@asynccontextmanager
async def lifespan(app: FastAPI):
    app.state.redis = redis.from_url(REDIS_URL)
    if VERSION == "v2":
        # A genuinely new, genuinely harmless template at deploy time -- the
        # novelty ranker has to *not* be fooled by this one.
        log.info("payments v2: fast-path validator enabled")
    log.info("payments %s ready", VERSION)
    yield
    await app.state.redis.aclose()


app = FastAPI(title=f"payments {VERSION}", lifespan=lifespan)
install(app)


def validate_v1(body: dict) -> tuple[float, str]:
    amount, currency = body.get("amount"), body.get("currency")
    if not isinstance(amount, (int, float)) or amount <= 0:
        raise ValueError("amount must be a positive number")
    if not isinstance(currency, str) or currency not in SUPPORTED:
        raise ValueError("currency not supported")
    return float(amount), currency


def validate_v2(body: dict) -> tuple[float, str]:
    """SEEDED REGRESSION (scenario S1).

    The v2 'fast-path' validator shipped with a USD-only currency table. Every
    other currency -- about a fifth of traffic -- raises out of the handler.
    """
    amount, currency = validate_v1(body)
    if currency != "USD":
        raise UnsupportedCurrency(
            f"payment validation failed: unsupported currency {currency} req={req_id_var.get()}")
    # Benign new INFO template on ~80% of traffic: a second novelty decoy.
    log.info("fast-path validation passed for currency %s", currency)
    return amount, currency


@app.post("/charge")
async def charge(request: Request):
    body = await request.json()
    try:
        amount, currency = (validate_v2 if VERSION == "v2" else validate_v1)(body)
    except ValueError as e:
        return JSONResponse({"error": str(e)}, status_code=400)
    rid = req_id_var.get()
    r: redis.Redis = request.app.state.redis
    cached = await r.get(f"charge:{rid}")
    if cached:
        return json.loads(cached)
    # Simulated processor latency, 20-60 ms, deterministic per request.
    await asyncio.sleep(0.02 + 0.04 * noise_roll(rid, "processor-latency"))
    result = {"charge_id": f"ch_{uuid.uuid5(uuid.NAMESPACE_URL, rid).hex[:16]}",
              "status": "approved", "amount": amount, "currency": currency, "version": VERSION}
    # Steady-state noise: a rare transient cache hiccup, retried and fine,
    # logged at ERROR the way a retried write failure usually is. It exists
    # so the failure template below is KNOWN-BUT-RARE before scenario S3
    # makes it burst -- novelty by first-sight must not be the only signal.
    # (The engine's template identity includes the level, so the hiccup
    # must share the level, not just the words, with the real failure.)
    if noise_roll(rid, "cache-hiccup") < CACHE_HICCUP_RATE:
        log.error("cache write failed: %s", "TimeoutError",
                  extra={"upstream": "redis", "detail": "transient; retried once, ok"})
    try:
        await r.setex(f"charge:{rid}", 300, json.dumps(result))
    except RedisError as e:
        # The idempotency record could not be written. Failing CLOSED is the
        # only safe answer for a payments service (a lost record is a double
        # charge on retry); the cache is configured `noeviction` for the same
        # reason. This is scenario S3's failure mode: the same template as
        # the hiccup above, at ERROR, with the store's own words in `detail`.
        log.error("cache write failed: %s", e.__class__.__name__,
                  extra={"upstream": "redis", "detail": str(e)[:200]})
        await _warn_memory_pressure(r)
        return JSONResponse({"error": "payment store unavailable", "req_id": rid}, status_code=503)
    return result


async def _warn_memory_pressure(r: redis.Redis) -> None:
    """At most once per 5 s: what the store says about its own memory."""
    global _pressure_warned_at
    now = time.monotonic()
    if now - _pressure_warned_at < 5.0:
        return
    _pressure_warned_at = now
    try:
        info = await r.info("memory")
        log.warning("redis memory pressure: used_memory %s maxmemory %s policy %s",
                    info.get("used_memory"), info.get("maxmemory"), info.get("maxmemory_policy"),
                    extra={"upstream": "redis"})
    except RedisError:
        pass


if __name__ == "__main__":
    run(app, 8082)
