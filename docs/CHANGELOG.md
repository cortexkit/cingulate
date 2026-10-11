# Changelog

## 0.3.0

- Pin Cerebellum's generic-consent-only admission and owner-scoped requester
  operations with real-handler coverage; identity remains daemon-attested.
- Add optional printable-ASCII `requester_ref` correlation for generic consent.
  Preserve it in durable requester records and feeds, exclude it from phone
  cards, and keep it independent of pending-card deduplication.
- Publish additive requester v2 vectors without changing any v1 bytes. Existing
  JSON records accept the absent field, so no store schema migration is needed.

## 0.2.0

- Add AFT-only tool permission asks, worker-to-head routing from daemon stamps,
  policy-limited core relays, escalation to the phone, first-settlement-wins and
  durable answer-attempt audit. Neither the supervising-session nor the user
  answer path expires.
- Add informational macOS folder permission cards, deduplicated while open.
- Add restart-safe call-key replay, principal-only withdrawal and read-only,
  re-awaitable waits with caller bounds.
- Track parent scope liveness through the daemon's `scope.describe`; route loss
  and unknown liveness never imply a head has ended.
- Deliver five structured core notices through a durable, deduplicated outbox.
  Retry transport failures without blocking mutations; record unclaimed results
  and treat definitive missing heads as named refusals.
- Migrate version-3 stores forward to version 4 without rewriting existing cards.
  Preserve existing contracts and corpora; publish requester and phone v2 plus
  a SHA256-pinned served permission corpus.
