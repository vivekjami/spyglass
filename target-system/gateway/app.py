"""gateway: the public edge. Forwards /checkout to orders.

Also the request-capture point: every checkout is logged with a sanitized
header subset and a capped body, so a failing request can later be replayed
verbatim (README C9). Auth-shaped headers are never captured, on principle --
the pattern has to survive contact with real data one day.
"""
from __future__ import annotations

import os
from contextlib import asynccontextmanager

import httpx
from fastapi import FastAPI, Request, Response
from fastapi.responses import JSONResponse

from common import install, log, noise_roll, req_id_var, run

ORDERS_URL = os.environ.get("ORDERS_URL", "http://orders:8081")
CAPTURED_HEADERS = {"content-type", "user-agent", "x-client-class", "x-request-id"}
BODY_CAP = 1024


@asynccontextmanager
async def lifespan(app: FastAPI):
    app.state.client = httpx.AsyncClient(timeout=httpx.Timeout(8.0))
    log.info("gateway ready")
    yield
    await app.state.client.aclose()


app = FastAPI(title="gateway", lifespan=lifespan)
install(app)


@app.post("/checkout")
async def checkout(request: Request):
    rid = req_id_var.get()
    raw = await request.body()
    log.info("request captured", extra={
        "kind": "request_capture", "method": "POST", "path": "/checkout",
        "headers": {k: v for k, v in request.headers.items() if k.lower() in CAPTURED_HEADERS},
        "body": raw[:BODY_CAP].decode("utf-8", "replace")})
    if noise_roll(rid, "gw-slow") < 0.02:  # steady WARN chatter
        log.warning("upstream latency above soft threshold", extra={"upstream": "orders"})
    try:
        r = await request.app.state.client.post(
            f"{ORDERS_URL}/orders", content=raw,
            headers={"content-type": request.headers.get("content-type", "application/json"),
                     "x-request-id": rid})
    except httpx.HTTPError as e:
        log.error("orders unreachable: %s", e.__class__.__name__, extra={"upstream": "orders"})
        return JSONResponse({"error": "orders unavailable", "req_id": rid}, status_code=503)
    if r.status_code >= 500:
        log.error("checkout failed: orders returned HTTP %d", r.status_code,
                  extra={"upstream": "orders", "status": r.status_code})
        return JSONResponse({"error": "checkout failed", "req_id": rid}, status_code=502)
    return Response(content=r.content, status_code=r.status_code, media_type="application/json")


if __name__ == "__main__":
    run(app, 8080)
