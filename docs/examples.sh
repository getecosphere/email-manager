#!/usr/bin/env bash
# email-manager LXS smoke test — golden request->response pairs.
# Usage: BASE_URL=<http://host:port> ./examples.sh
# Runs against a pulled binary or a live estate URL; every curl must succeed
# and return the documented shape or the script exits non-zero.
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:8085}"
TO="${SMOKE_TO:-smoke-test@example.com}"

echo "BASE_URL=$BASE_URL"

# 1) health (bare + gateway alias) — JSON body
for path in /health /api/email/health; do
  code=$(curl -s -o /tmp/em-health.out -w '%{http_code}' "$BASE_URL$path")
  test "$code" = "200"
  rg -q '"status":"ok"' /tmp/em-health.out
  echo "OK $path -> 200 JSON"
done

# 2) enqueue single -> 202 { id }
code=$(curl -s -o /tmp/em-send.out -w '%{http_code}' -X POST "$BASE_URL/api/email/send" \
  -H 'Content-Type: application/json' \
  -d "{\"to\":\"$TO\",\"name\":\"Smoke\",\"subject\":\"Smoke test\",\"html\":\"<p>hi</p>\",\"campaign\":\"smoke\"}")
test "$code" = "202"
ID=$(rg -o '"id":"[^"]+"' /tmp/em-send.out | head -1 | cut -d'"' -f4)
test -n "$ID"
echo "OK POST /api/email/send -> 202 ($ID)"

# 3) status -> 200 (body must echo the id)
code=$(curl -s -o /tmp/em-status.out -w '%{http_code}' "$BASE_URL/api/email/status/$ID")
test "$code" = "200"
rg -q "$ID" /tmp/em-status.out
echo "OK GET /api/email/status/:id -> 200"

# 4) invalid email -> 400 plain text
code=$(curl -s -o /tmp/em-invalid.out -w '%{http_code}' -X POST "$BASE_URL/api/email/send" \
  -H 'Content-Type: application/json' \
  -d '{"to":"not-an-email","subject":"x","html":"<p>x</p>"}')
test "$code" = "400"
rg -q "invalid recipient email" /tmp/em-invalid.out
echo "OK invalid email -> 400 plain text"

# 5) missing subject/html -> 400
code=$(curl -s -o /tmp/em-empty.out -w '%{http_code}' -X POST "$BASE_URL/api/email/send" \
  -H 'Content-Type: application/json' \
  -d "{\"to\":\"$TO\",\"subject\":\"\",\"html\":\"\"}")
test "$code" = "400"
rg -q "subject and html are required" /tmp/em-empty.out
echo "OK empty subject/html -> 400"

# 6) batch of 501 -> 400
big=$(python3 -c "import json;print(json.dumps({'messages':[{'to':'$TO','subject':'s','html':'<p>x</p>'}]}*501))")
code=$(curl -s -o /tmp/em-batch.out -w '%{http_code}' -X POST "$BASE_URL/api/email/batch" \
  -H 'Content-Type: application/json' -d "$big")
test "$code" = "400"
rg -q "batch is capped at 500" /tmp/em-batch.out
echo "OK 501-message batch -> 400"

# 7) batch of 2 -> 200 { ids: [...] }
code=$(curl -s -o /tmp/em-batch2.out -w '%{http_code}' -X POST "$BASE_URL/api/email/batch" \
  -H 'Content-Type: application/json' \
  -d "{\"messages\":[{\"to\":\"$TO\",\"subject\":\"s1\",\"html\":\"<p>a</p>\"},{\"to\":\"$TO\",\"subject\":\"s2\",\"html\":\"<p>b</p>\"}]}")
test "$code" = "200"
rg -q '"ids":\["' /tmp/em-batch2.out
echo "OK POST /api/email/batch -> 200"

# 8) status of unknown id -> 404 plain text
code=$(curl -s -o /tmp/em-notfound.out -w '%{http_code}' "$BASE_URL/api/email/status/00000000-0000-0000-0000-000000000000")
test "$code" = "404"
rg -q "message not found" /tmp/em-notfound.out
echo "OK unknown status id -> 404"

# 9) unsubscribe -> 200 { suppressed: true }
code=$(curl -s -o /tmp/em-unsub.out -w '%{http_code}' -X POST "$BASE_URL/api/email/unsubscribe" \
  -H 'Content-Type: application/json' \
  -d "{\"email\":\"$TO\",\"reason\":\"smoke test\"}")
test "$code" = "200"
rg -q '"suppressed":true' /tmp/em-unsub.out
echo "OK POST /api/email/unsubscribe -> 200"

# 10) sending to the now-suppressed address -> 422
code=$(curl -s -o /tmp/em-suppressed.out -w '%{http_code}' -X POST "$BASE_URL/api/email/send" \
  -H 'Content-Type: application/json' \
  -d "{\"to\":\"$TO\",\"subject\":\"x\",\"html\":\"<p>x</p>\"}")
test "$code" = "422"
rg -q "recipient is suppressed" /tmp/em-suppressed.out
echo "OK suppressed recipient -> 422"

echo "email-manager LXS smoke test PASSED"
