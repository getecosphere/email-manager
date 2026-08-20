# email-manager API

Base path: `/api/email` (plus bare `/health`). Auth: **none** — no endpoint
requires or verifies authentication. Errors: plain-text body (the error
message string, e.g. `invalid recipient email`), **not** JSON. The single
exception is `GET /health` + `GET /api/email/health`, which return JSON.

## Endpoints

### GET /health
- **Purpose:** Liveness + provider/queue state probe (also mounted at
  `/api/email/health`).
- **Auth required:** no
- **Success 200:** JSON
  ```json
  { "status": "ok", "provider_configured": true, "queued": 3 }
  ```
  `provider_configured` is `false` when `BREVO_API_KEY` or `MAIL_FROM_EMAIL`
  is empty (worker pauses). `queued` counts messages with status
  `queued`/`sending`.
- **Errors:** none

### GET /api/email/health
- **Purpose:** Alias of `/health` for the estate gateway.
- **Auth required:** no
- **Success 200:** same JSON as `/health`.

### POST /api/email/send
- **Purpose:** Enqueue one transactional email. Returns 202 immediately; the
  worker sends it asynchronously.
- **Auth required:** no
- **Body params:**
  | Param | Type | Required | Notes |
  |---|---|---|---|
  | `to` | string | yes | normalized (trim + lowercase); must pass validation |
  | `name` | string | no | default `""` |
  | `subject` | string | yes | non-empty (after trim) |
  | `html` | string | yes | non-empty (after trim); plain text not supported |
  | `campaign` | string | no | default `"transactional"` |
  | `cc` | `EmailRecipient[]` | no | `{ "email", "name" }`; invalid addresses rejected, duplicates removed, and the primary `to` address omitted |
  | `attachments` | `EmailAttachment[]` | no | `{ "name", "content" }`, where `content` is base64; max 5 files and 10 MB decoded total |
- **Success 202:** `{ "id": "<uuid>" }`
- **Errors:**
  - 400 — `invalid recipient email` (see validation rules in gotchas) or
    `subject and html are required`; invalid CC, attachment names, or base64
    payloads are also rejected
  - 422 — `recipient is suppressed (unsubscribed or hard bounced)`
  - 413 — more than 5 attachments or more than 10 MB decoded attachment data
  - 500 — `Email storage is having trouble.` (MongoDB failure)

### POST /api/email/batch
- **Purpose:** Enqueue up to 500 emails in one request.
- **Auth required:** no
- **Body params:**
  | Param | Type | Required | Notes |
  |---|---|---|---|
  | `messages` | `SendRequest[]` | yes | max 500 per request |
- **Success 200:** `{ "ids": ["<uuid>", "..."] }` — one id per message, in
  input order.
- **Errors:**
  - 400 — `batch is capped at 500 messages per request`, or the same
    validation errors as `/send` for any message (the whole request fails)
  - 422 — a message's recipient is suppressed
  - 500 — MongoDB failure

### GET /api/email/status/:id
- **Purpose:** Fetch delivery status for one queued message.
- **Auth required:** no
- **Path param:** `id` — the uuid returned by `/send` or `/batch`.
- **Success 200:** JSON
  ```json
  {
    "id": "7f3a...",
    "to": "user@example.com",
    "name": "User",
    "cc": [{ "email": "pic@example.com", "name": "PIC Sekolah" }],
    "subject": "Welcome",
    "html": "<p>Hi</p>...",
    "campaign": "welcome",
    "attachments": ["report.pdf"],
    "status": "sent",
    "attempts": 1,
    "error": null,
    "created_at": "2026-08-13T07:00:00.000Z",
    "sent_at": "2026-08-13T07:00:01.000Z",
    "next_attempt_at": "2026-08-13T07:00:00.000Z"
  }
  ```
  `attachments` contains file names only; base64 content is never returned and
  is discarded from storage after a successful provider submission. `status`
  is one of: `queued`, `sending`, `sent`, `delivered`, `bounced`,
  `spam_reported`, `suppressed`, `failed`. `error` is `null` when no failure.
- **Errors:**
  - 404 — `message not found`
  - 500 — `Email storage is having trouble.`

### POST /api/email/unsubscribe
- **Purpose:** Add an address to the suppression list and mark any of its
  queued/sending/failed messages as `suppressed`.
- **Auth required:** no
- **Body params:**
  | Param | Type | Required | Notes |
  |---|---|---|---|
  | `email` | string | yes | normalized; must pass validation |
  | `reason` | string | no | default `"user_requested"` |
- **Success 200:** `{ "suppressed": true }`
- **Errors:**
  - 400 — `invalid email`
  - 500 — `Email storage is having trouble.`

## Error reference

Errors are plain-text bodies (an `(StatusCode, String)` pair):

| Status | Body | When |
|---|---|---|
| 400 | `invalid recipient email` / `invalid email` | email fails validation |
| 400 | `subject and html are required` | send with empty subject or html |
| 400 | `invalid cc recipient email` | a CC address fails validation |
| 400 | `invalid attachment name` / `attachment content must be valid base64` | malformed attachment |
| 400 | `batch is capped at 500 messages per request` | more than 500 messages |
| 413 | `at most 5 attachments are allowed` / `attachments exceed the 10 MB limit` | attachment limit exceeded |
| 404 | `message not found` | unknown status id |
| 422 | `recipient is suppressed (unsubscribed or hard bounced)` | enqueue to a suppressed address |
| 500 | `Email storage is having trouble.` | MongoDB operation failed |

## Rate limiting / limits

Not enforced at the API layer (no per-IP throttling). The internal worker
enforces a **global hourly budget** (default 400, `EMAIL_GLOBAL_PER_HOUR`),
ramped by the warm-up schedule (default `EMAIL_WARMUP_MAX_PER_HOUR=400` over
`EMAIL_WARMUP_DAYS=5`); when the budget for the current hour is exhausted the
worker sends nothing that tick. A per-recipient daily counter
(`EMAIL_PER_RECIPIENT_DAY`, default 5) is recorded but **not enforced** in the
send path (see gotchas). The worker runs every 2 seconds and processes at most
20 messages per tick.
