# email-manager domain

Reusable transactional email domain. Owns outbound delivery for an estate:
queueing, rate limiting, anti-spam, suppression, and per-message status.

## Contract

- `POST /api/email/send` — enqueue one email
  `{ "to", "name", "subject", "html", "campaign" }` → `{ "id" }`
- `POST /api/email/batch` — `{ "messages": [...] }` same shape → `{ "ids": [...] }`
- `GET /api/email/status/:id` — `{ id, status, to, subject, attempts, error, createdAt, sentAt }`
- `POST /api/email/unsubscribe` — `{ "email", "reason" }` → adds to suppression list
- `GET /api/email/health` — reports provider + queue state
- Internal worker drains the queue, enforces rate caps, sends via Brevo.
- Ownership: sending, deliverability, suppression. Never owns credentials
  (BREVO_API_KEY is operator-set in .env), never owns a UI.
- Rate caps: per-recipient/day (default 5), global/hour (default 400),
  warm-up ramp for new sender domains (default 50/hour to 400 over 5 days).
- Suppression: hard bounces and spam reports auto-suppress; unsubscribe is
  explicit. Suppressed recipients are never sent to.

## Runtime

`backend/` is Rust + MongoDB. `backend/.env.example` declares config.
BREVO_API_KEY is set by the operator in the generated .env on the CT.

## Composition

Compose with `eco compose add email-manager`. Eco declares
`email-manager-backend` (rust, mongodb). Consumers declare
`EMAIL_MANAGER_URL=` in their `.env.example`; Eco resolves it to the
internal backend URL. The estate gateway routes `/api/email/*`.
