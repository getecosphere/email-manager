# email-manager — LXS docs

## Capability

Reusable transactional email domain. Owns outbound delivery for an estate:
queueing, rate limiting (global hourly budget + warm-up ramp), anti-spam
suppression, and per-message status. Consumers enqueue HTML emails and poll
delivery state; an internal worker drains the queue and sends via **Brevo**
(`POST https://api.brevo.com/v3/smtp/email`). Pick this LXS when a composed
estate needs to send transactional email (notifications, receipts,
verifications) with deliverability care and an unsubscribe/suppression list.

## What it owns / never owns

- **Owns:** the outbound queue (`messages` collection), delivery status,
  suppression list (`suppressions`), rate counters (`rate_counters`),
  provider integration (Brevo), auto-appended unsubscribe footer + headers.
- **Never owns:** credentials — `BREVO_API_KEY` is operator-set in `.env`.
  A UI, inbound/receiving email, or template authoring.

## Compose it

```yaml
# ecompose.yml
services:
  email-manager-backend:
    lxs: email-manager@1.0.1
    grants:
      secrets: [SERVER_PORT, API_BASE_PATH, MONGODB_URI, BREVO_API_KEY, MAIL_FROM_EMAIL, MAIL_FROM_NAME]
```

## Quick usage

```bash
BASE=http://127.0.0.1:8085

# enqueue one email -> 202 { "id": ... }
curl -s -X POST "$BASE/api/email/send" -H 'Content-Type: application/json' \
  -d '{"to":"user@example.com","name":"User","subject":"Welcome","html":"<p>Hi</p>","campaign":"welcome"}'

# poll its status -> 200
curl -s "$BASE/api/email/status/<id>"

# health: provider configured + queue depth
curl -s "$BASE/api/email/health"
```

No auth on any endpoint; errors are plain-text strings (not JSON), except
`/health` which is JSON.

## Docs index

- `api.md` — full endpoint reference with request/response JSON and errors
- `examples.sh` — executable smoke test (golden request→response pairs)
- `openapi.json` — machine-readable OpenAPI 3.0 spec
- `changelog.md` — version history + breaking changes
- `gotchas.md` — production-learned constraints and operational gotchas

## For AI agents

This LXS is distributed as a **binary only** — these docs are the entire
interface. Match `api.md` shapes exactly; run `examples.sh` against a pulled
binary or live estate URL before trusting behavior. See
`docs/gotchas.md` for constraints that are invisible in the binary.
