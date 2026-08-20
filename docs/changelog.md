# Changelog

## 2.1.0 (2026-08-20)
- Add optional CC recipients and base64 file attachments to single and batch
  email requests.
- Validate attachment count, decoded size, filename, and base64 encoding.
- Return attachment filenames only from status and discard attachment contents
  after successful provider submission.

## 1.0.2 (2026-08-20)
- Artifacts: added `darwin/arm64` for local `eco up dev`.

## 2.0.0 (2026-08-19)
- Logging contract: service logs now emitted as newline-delimited JSON (NDJSON) to stdout per the platform LXS logging contract (`ts`/`level`/`msg` + optional `service`,`request_id`,`status`,`latency_ms`,`user_id`,`error`). Breaking change — log output format changed.

## 1.0.1 (next release)
- Store rate-counter window and message datetimes consistently as RFC 3339
  strings for reliable queries (`bdffc3b`, `70dce31`)

## 1.0.0
- initial release — LXS manifest `email-manager@1.0.0` (`93c8596`). Pre-manifest
  development history:
  - Initial reusable domain: Brevo outbound, anti-spam queue, suppression,
    rate caps, warm-up ramp (`9c239eb`)
  - Single-binary composition via `bootstrap()` (`b5dbf3b`)
