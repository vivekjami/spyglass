"""fraudcheck: a third-party fraud-scoring vendor, as seen from inside the
target system -- i.e. barely.

orders calls it synchronously before every charge. It is deliberately
UNOBSERVED: it writes no request logs to the shared log directory, exposes
no metrics the engine scrapes, and publishes no host port. The topology
knows it exists (every real system has a dependency like this); the
telemetry does not know what it does. That gap is what scenarios S2 and S6
are built on:

  /v1/score   the integration in production: ~3 ms
  /v2/score   the vendor's newer API: synchronous scoring, ~1.5 s per call,
              ~9 s "deep scoring" for premium/corporate cards (S2 switches
              orders to it by config)
  /knobs/fraudcheck.json {"degrade": {"share": 0.12, "latency_ms": 9000}}
              the vendor degrading with no change on our side (S6)
"""
from __future__ import annotations

import asyncio
import os

from fastapi import FastAPI, Request

from common import knob, noise_roll

V2_BASE_MS = float(os.environ.get("FRAUD_V2_BASE_MS", "1500"))
V2_DEEP_MS = float(os.environ.get("FRAUD_V2_DEEP_MS", "9000"))
DEEP_CLASSES = {"premium", "corporate"}

app = FastAPI(title="fraudcheck (external vendor)")


@app.get("/health")
async def health():
    return {"ok": True, "service": "fraudcheck"}


async def _degradation(rid: str) -> float:
    d = knob("fraudcheck").get("degrade") or {}
    share, ms = float(d.get("share", 0.0)), float(d.get("latency_ms", 0.0))
    if share > 0 and ms > 0 and noise_roll(rid, "fraud-degrade") < share:
        return ms
    return 0.0


@app.post("/v1/score")
async def score_v1(request: Request):
    rid = request.headers.get("x-request-id", "-")
    await asyncio.sleep(max(0.003, await _degradation(rid) / 1000))
    return {"score": round(noise_roll(rid, "fraud-score"), 3), "decision": "allow", "api": "v1"}


@app.post("/v2/score")
async def score_v2(request: Request):
    rid = request.headers.get("x-request-id", "-")
    body = await request.json()
    ms = V2_DEEP_MS if body.get("card_class") in DEEP_CLASSES else V2_BASE_MS
    await asyncio.sleep(max(ms, await _degradation(rid)) / 1000)
    return {"score": round(noise_roll(rid, "fraud-score"), 3), "decision": "allow", "api": "v2"}


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=int(os.environ.get("PORT", 8090)), log_config=None, access_log=False)
