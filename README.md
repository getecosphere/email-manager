# email-manager

Reusable transactional email domain for eco estates. Owns outbound email —
queueing, rate limiting, anti-spam, delivery tracking, and suppression — so
other domains never talk to a mail provider directly.

## Contract

- **Public API**
  - `POST /api/email/send` — enqueue one transactional email
    `{ "to": "...", "name": "...", "subject": "...", "html": "...", "campaign": "...", "cc": [...], "attachments": [...] }`
  - `POST /api/email/batch` — enqueue many emails at once
  - `GET /api/email/status/:id` — delivery status of a queued message
  - `POST /api/email/unsubscribe` — `{ "email": "...", "reason": "..." }`
  - `GET /api/email/health` — liveness + provider config check
- **Owns deliverability.** All sending goes through Brevo's transactional
  API. The domain enforces per-recipient rate caps, a global hourly budget,
  a suppression list (hard bounces, spam reports, unsubscribes), and a
  warm-up ramp so bulk sends don't get flagged as spam.
- **Never owns credentials.** `BREVO_API_KEY`, `MAIL_FROM_EMAIL`,
  `MAIL_FROM_NAME` come from the service `.env` (Eco-managed), never code,
  never git.
- **Queue, not fire-and-forget.** `send`/`batch` enqueue into MongoDB; a
  background worker drains the queue at the configured rate and records
  per-message status. Consumers get an `id` back and can poll status.
- **Degrades safely.** If Brevo is down, messages stay queued and retry with
  backoff. If the provider rejects a message (bounce/spam), the recipient is
  suppressed so future sends skip it automatically.

## Runtime

`backend/` is Rust (axum) + MongoDB. Config lives in `backend/.env.example`;
secrets (`BREVO_API_KEY`) are operator-set in the generated `.env` on the CT.

## Composition

Compose with `eco compose add email-manager` after the domain is in Eco's
catalog. Eco declares `email-manager-backend` (Rust + MongoDB). Consumers
declare `EMAIL_MANAGER_URL=` in their `.env.example` and Eco resolves it to
`http://127.0.0.1:<port>/api/email` in dev and prod alike (backend-to-backend
traffic stays internal). The estate gateway routes `/api/email/*` to it.
