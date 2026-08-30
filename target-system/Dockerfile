# One image for every Python service in the target system; the Compose
# `command` selects which one runs. The target is scenery, not the show.
FROM python:3.12-slim
WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY common ./common
COPY gateway ./gateway
COPY orders ./orders
COPY payments ./payments
COPY loadgen ./loadgen
COPY fraudcheck ./fraudcheck
ENV PYTHONUNBUFFERED=1 PYTHONPATH=/app
