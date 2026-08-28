"""orders: persists an order, then charges it via payments.

Routes to payments-v1 or payments-v2 per the deployer's current.json, read on
every request -- so a deploy or rollback takes effect on the next order with
no restart. That is what makes both payments versions "always on".
"""
from __future__ import annotations

import asyncio
import os
import time
import uuid
from contextlib import asynccontextmanager

import asyncpg
import httpx
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

from common import SERVICE, UPSTREAM, current_deploy, install, log, noise_roll, req_id_var, run

PAYMENTS = {
    "v1": os.environ.get("PAYMENTS_V1_URL", "http://payments-v1:8082"),
    "v2": os.environ.get("PAYMENTS_V2_URL", "http://payments-v2:8082"),
}
DATABASE_URL = os.environ.get("DATABASE_URL", "postgresql://spyglass:spyglass@postgres:5432/spyglass")


@asynccontextmanager
async def lifespan(app: FastAPI):
    for attempt in range(30):
        try:
            app.state.pool = await asyncpg.create_pool(DATABASE_URL, min_size=1, max_size=5)
            break
        except Exception as e:  # postgres may still be starting
            log.warning("postgres not ready (attempt %d): %s", attempt + 1, e.__class__.__name__)
            await asyncio.sleep(1)
    else:
        raise RuntimeError("postgres unavailable")
    async with app.state.pool.acquire() as c:
        await c.execute("""CREATE TABLE IF NOT EXISTS orders (
            order_id TEXT PRIMARY KEY, req_id TEXT, customer TEXT,
            amount NUMERIC, currency TEXT, created_at TIMESTAMPTZ DEFAULT now())""")
    app.state.client = httpx.AsyncClient(timeout=httpx.Timeout(5.0))
    log.info("orders ready")
    yield
    await app.state.client.aclose()
    await app.state.pool.close()


app = FastAPI(title="orders", lifespan=lifespan)
install(app)


@app.post("/orders")
async def create_order(request: Request):
    rid = req_id_var.get()
    try:
        body = await request.json()
    except Exception:
        return JSONResponse({"error": "invalid json"}, status_code=400)
    amount, currency = body.get("amount"), body.get("currency")
    customer = body.get("customer", "anon")
    if not isinstance(amount, (int, float)) or amount <= 0 or not isinstance(currency, str):
        log.info("order rejected: invalid payload")
        return JSONResponse({"error": "amount and currency required"}, status_code=400)

    order_id = f"ord_{uuid.uuid5(uuid.NAMESPACE_URL, rid).hex[:12]}"
    t0 = time.perf_counter()
    async with request.app.state.pool.acquire() as c:
        await c.execute(
            "INSERT INTO orders(order_id, req_id, customer, amount, currency) "
            "VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING",
            order_id, rid, customer, amount, currency)
    pg_ms = (time.perf_counter() - t0) * 1000
    if noise_roll(rid, "pg-slow") < 0.03:  # steady WARN chatter, part of the noise profile
        log.warning("postgres insert slower than budget",
                    extra={"latency_ms": round(pg_ms + 80 + 60 * noise_roll(rid, "pg-slow-amt"), 1)})

    route = current_deploy("payments")
    ver = route["version"]
    url = PAYMENTS.get(ver)
    if url is None:
        log.error("no payments endpoint for version %s", ver, extra={"deploy_id": route["deploy_id"]})
        return JSONResponse({"error": "routing misconfigured"}, status_code=500)
    try:
        resp = await request.app.state.client.post(
            f"{url}/charge",
            json={"amount": amount, "currency": currency, "customer": customer,
                  "card_class": body.get("card_class", "standard")},
            headers={"x-request-id": rid})
    except httpx.HTTPError as e:
        UPSTREAM.labels(SERVICE, "payments", ver, "exception").inc()
        log.error("payments unreachable: %s", e.__class__.__name__,
                  extra={"upstream": "payments", "upstream_version": ver, "deploy_id": route["deploy_id"]})
        return JSONResponse({"error": "payment service unavailable", "order_id": order_id}, status_code=503)
    UPSTREAM.labels(SERVICE, "payments", ver, str(resp.status_code)).inc()
    if resp.status_code >= 500:
        log.error("payments charge failed with HTTP %d", resp.status_code,
                  extra={"upstream": "payments", "upstream_version": ver,
                         "status": resp.status_code, "deploy_id": route["deploy_id"]})
        return JSONResponse({"error": "payment failed", "order_id": order_id,
                             "payments_version": ver}, status_code=502)
    if resp.status_code >= 400:
        return JSONResponse({"error": "payment rejected", "order_id": order_id,
                             "detail": resp.json()}, status_code=422)
    data = resp.json()
    log.info("order placed", extra={"upstream": "payments", "upstream_version": ver})
    return {"order_id": order_id, "charge_id": data["charge_id"], "payments_version": ver}


if __name__ == "__main__":
    run(app, 8081)
