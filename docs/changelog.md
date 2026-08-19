# Changelog

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
