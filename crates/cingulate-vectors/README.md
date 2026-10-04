# Consent wire vectors

This test-only workspace crate compares persisted record projections and actual
in-process handler replies and notifier payloads against the vendored vectors.
All stores are scratch SQLite stores; the injected clock is fixed unless the
scenario advances it, and the sweep timer is disabled.

- `records.rs` covers every phone projection, including the verbatim worker
  subject, and delivery/retraction pointers with and without a machine id.
- `requester.rs` follows the upstream README scenario and its refusal-table order.
  It adapts transport route fields to an internal stamp, maps `elicitation.*` to
  `consent.*`, and excludes `elicitation_unavailable`, which cingulate never emits.
- `consent.rs` pins cingulate's additive fields and the console refusal without
  removing anything from the parsed reply or pointer.
- `pins.rs` pins each SHA256SUMS and verifies complete file coverage and bytes.

Comparisons remove only `requesterLabel` and `atConsole` from upstream phone
projections, or `requester_label` from upstream requester records. Opaque nested
JSON is never filtered. Placeholder substitution preserves numeric clock values
as JSON integers. No test regenerates fixtures from the implementation.
