#!/usr/bin/env bash
# Register a model provider with the running TrueForge instance.
#
# Reads MODEL_PROVIDER / MODEL_API_KEY from .env (gitignored), so the key never
# reaches shell history, the repo, or a command line in a screen recording.
#
#   cp .env.example .env && $EDITOR .env
#   scripts/configure-model.sh
#
# The provider manifest requires an explicit `models` list, so we take the
# provider's entry from TrueForge's own catalog and register every model it
# offers — which also gives the Model Generalization Experiment its Model B
# for free.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/scripts/env.sh" >/dev/null

[ -f "$ROOT/.env" ] || { echo "error: .env not found; copy .env.example to .env" >&2; exit 1; }
# shellcheck source=/dev/null
set -a; source "$ROOT/.env"; set +a
: "${MODEL_PROVIDER:?set MODEL_PROVIDER in .env}"
: "${MODEL_API_KEY:?set MODEL_API_KEY in .env}"

catalog="$(curl -sf "$TRUEFORGE_URL/api/v1/catalogs/model-providers")" \
  || { echo "error: cannot reach $TRUEFORGE_URL — is the harness running?" >&2; exit 1; }

payload="$(MODEL_PROVIDER="$MODEL_PROVIDER" python3 - "$catalog" <<'PY'
import json, os, sys
prov_type = os.environ["MODEL_PROVIDER"]
catalog = json.loads(sys.argv[1])["data"]
entry = next((p for p in catalog if p["type"] == prov_type), None)
if entry is None:
    sys.exit(f"provider '{prov_type}' not in catalog: "
             + ", ".join(p["type"] for p in catalog))
models = [{"model_id": m["model_id"], "name": m["name"], "properties": m["properties"]}
          for m in entry.get("models", [])]
if not models:
    sys.exit(f"provider '{prov_type}' lists no models; use a 'custom' provider instead")
print(json.dumps({"manifest": {"type": prov_type,
                               "auth": {"api_key": "__API_KEY__"},
                               "models": models}}))
PY
)"

# Substitute the key only at the last moment, via stdin, so it is never an argv.
body="$(MODEL_API_KEY="$MODEL_API_KEY" python3 -c '
import os, sys
sys.stdout.write(sys.stdin.read().replace("__API_KEY__", os.environ["MODEL_API_KEY"]))' <<<"$payload")"

code="$(printf '%s' "$body" | curl -s -o /tmp/tf-provider.out -w '%{http_code}' \
  -X POST "$TRUEFORGE_URL/api/v1/settings/model-providers" \
  -H 'content-type: application/json' --data-binary @-)"

if [ "$code" != "200" ] && [ "$code" != "201" ]; then
  echo "provider registration failed (HTTP $code):" >&2
  sed -E 's/(sk-|sk-ant-)[A-Za-z0-9_-]+/\1***/g' /tmp/tf-provider.out >&2
  exit 1
fi

echo "provider '$MODEL_PROVIDER' registered. models available for chat:"
curl -s "$TRUEFORGE_URL/api/v1/models" | python3 -c '
import sys, json
for m in json.load(sys.stdin).get("data", []):
    print("  -", m.get("name") or m.get("model_id"))'
