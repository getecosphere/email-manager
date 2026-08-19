# Gotchas

Production-learned constraints not visible in the binary:

- **No authentication on any endpoint.** Anyone who can reach the API can
  enqueue mail, read status, or unsubscribe addresses. An estate must put the
  API behind an authenticated gateway.
- **Errors are plain-text, not JSON.** Every non-2xx response body is a bare
  string (`invalid recipient email`, `message not found`, ...) except
  `/health`. Consumers must handle text error bodies, not parse `{error: ...}`.
- **Per-recipient daily cap is tracked but NOT enforced.** `EMAIL_PER_RECIPIENT_DAY`
  (default 5) increments a `recipient-day-<email>` counter after each send, but
  the worker never consults it to skip a recipient — only the **global hourly
  budget** gates sending (`EMAIL_GLOBAL_PER_HOUR`, default 400, ramped by
  warm-up). README.md advertises a per-recipient/day default of 5; the code
  only records it.
- **Warm-up ramp is calendar-based, not per-domain.** `warmup_rate` uses the
  day-of-year (`now.ordinal() % warmup_days`) — a new verified domain ramps
  `EMAIL_WARMUP_MAX_PER_HOUR`/`EMAIL_WARMUP_DAYS` from 1 up to the max in
  roughly `EMAIL_WARMUP_DAYS` steps, but the phase resets every year and is
  shared with the global budget; it is not "days since the domain was
  verified".
- **Worker cadence and batching.** A tick runs every 2 seconds and sends at
  most 20 messages per tick, capped by the remaining hourly budget. When
  `BREVO_API_KEY` or `MAIL_FROM_EMAIL` is empty the worker pauses entirely
  (`health.provider_configured` shows `false`), so enqueued messages sit as
  `queued`.
- **Retry + backoff.** On send failure: `attempts + 1`, backoff
  `30s * attempts` capped at 1800s (30 min), retried as `queued` until
  `attempts >= 5`, then marked `failed`. After 5 failures the message stays
  `failed` with `error` set.
- **Permanent Brevo rejections auto-suppress.** A Brevo error with status 400,
  or whose body contains `invalid`, `bounce`, or `suppress`, writes the
  recipient to `suppressions` (reason `brevo_rejected:<first 80 chars>`);
  hard bounces / spam reports arriving via Brevo are expected to be handled
  the same way. Suppressed recipients are refused at enqueue (422) and skipped
  by the worker (marked `suppressed`).
- **Provider integration is Brevo-only.** Sending is a hard-coded
  `POST https://api.brevo.com/v3/smtp/email` with the `api-key` header; the
  estate needs outbound HTTPS. No SMTP fallback.
- **Unsubscribe footer is auto-appended** to every message: an inline
  `<...>/unsubscribe?email=<to>` block plus a `List-Unsubscribe` mail header,
  using `EMAIL_MANAGER_PUBLIC_URL` (default `https://eco.stuff8.com`). A wrong
  or unreachable public URL means every recipient sees a broken unsubscribe
  link. The `X-Eco-Campaign` header carries the campaign id.
- **Validation rules for `to`:** after trim + lowercase, must have a non-empty
  local part, a domain containing `.` with length ≥ 4, no spaces, and no `..`
  anywhere. `foo@bar` (no dot) is rejected.
- **HTML-only.** `html` is required and non-empty; plain-text content is not
  accepted. Subject/body length is not capped.
- **Datetimes are RFC 3339 strings** in Mongo (`created_at`, `sent_at`,
  `next_attempt_at`, rate-counter `window`) — fixed in 1.0.1. Do not feed
  integer epoch values.
- **Port precedence:** binds `PORT`, falling back to `SERVER_PORT`, then
  `8085`. The lxs.yml contract declares `SERVER_PORT`, so set `SERVER_PORT` for
  the composed service.
- **`API_BASE_PATH` is declared but ignored** in code — routes are hardcoded
  `/api/email/...` plus bare `/health`.
- **Suppression is by normalized email** (trim + lowercase). Addresses are
  stored normalized; mixed-case duplicates collapse to one suppression.
- **Batch is all-or-nothing:** any invalid/suppressed message in a batch fails
  the entire request (already-enqueued earlier messages in the batch remain
  enqueued; no rollback).
