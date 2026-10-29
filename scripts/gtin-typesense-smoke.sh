#!/usr/bin/env bash
# Smoke: seed-form GTIN exact filter against a throwaway Typesense (--rm).
set -euo pipefail
IMAGE="${TYPESENSE_IMAGE:-typesense/typesense:30.2}"
NAME="braian-gtin-smoke-$$"
KEY="smoke-test-key"
PORT="${TYPESENSE_SMOKE_PORT:-8109}"
cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT
docker run -d --rm --name "$NAME" -p "${PORT}:8108" "$IMAGE" \
  --data-dir=/tmp --api-key="$KEY" --enable-cors >/dev/null
for i in $(seq 1 40); do
  if curl -sf "http://127.0.0.1:${PORT}/health" >/dev/null; then break; fi
  sleep 0.25
done
curl -sf "http://127.0.0.1:${PORT}/health" >/dev/null
curl -sf -X POST "http://127.0.0.1:${PORT}/collections" \
  -H "X-TYPESENSE-API-KEY: $KEY" -H "Content-Type: application/json" \
  -d '{"name":"articles","fields":[{"name":"gtin","type":"string","facet":true},{"name":"key","type":"string"},{"name":"type_key","type":"string","facet":true}]}' >/dev/null
printf '%s\n' \
  '{"id":"22","gtin":"07310000000024","key":"ART-CHED-200","type_key":"article"}' \
  '{"id":"23","gtin":"07310000000031","key":"ART-CHED-400","type_key":"article"}' \
| curl -sf -X POST "http://127.0.0.1:${PORT}/collections/articles/documents/import?action=upsert" \
  -H "X-TYPESENSE-API-KEY: $KEY" -H "Content-Type: text/plain" --data-binary @- >/dev/null
FILTER='type_key:=article && gtin:=[`07310000000024`,`7310000000024`]'
RESP=$(curl -sf "http://127.0.0.1:${PORT}/collections/articles/documents/search" \
  -H "X-TYPESENSE-API-KEY: $KEY" \
  --get --data-urlencode "q=*" --data-urlencode "query_by=key" --data-urlencode "filter_by=${FILTER}")
echo "$RESP" | grep -q "ART-CHED-200"
echo "$RESP" | grep -q "07310000000024"
PREFIX='type_key:=article && gtin:=[`073100000`]'
PREF=$(curl -sf "http://127.0.0.1:${PORT}/collections/articles/documents/search" \
  -H "X-TYPESENSE-API-KEY: $KEY" \
  --get --data-urlencode "q=*" --data-urlencode "query_by=key" --data-urlencode "filter_by=${PREFIX}")
echo "$PREF" | grep -q '"found":0'
echo "gtin typesense smoke ok"
