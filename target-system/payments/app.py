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
import uuid
from contextlib import asynccontextmanager

import redis.asyncio as redis
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

from common import VERSION, install, log, noise_roll, req_id_var, run

SUPPORTED = {"USD", "EUR", "GBP", "JPY", "CAD", "AUD"}
REDIS_URL = os.environ.get("REDIS_URL", "redis://redis:6379/0")


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
    await r.setex(f"charge:{rid}", 300, json.dumps(result))
    return result


if __name__ == "__main__":
    run(app, 8082)
