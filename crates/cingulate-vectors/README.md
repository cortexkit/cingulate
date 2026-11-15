# Consent wire vectors

This test-only workspace crate compares persisted record projections and actual
in-process handler replies and notifier payloads against the checked-in vectors
in `test-vectors/`. Every value in those vectors is invented test data.
All stores are scratch SQLite stores; the injected clock is fixed unless the
scenario advances it, and the sweep timer is disabled.

- `records.rs` covers every phone projection, including the verbatim worker
  subject, and delivery/retraction pointers with and without a machine id.
- `requester.rs` follows the `elicitation-requester-v1` README scenario and its refusal-table order.
  It adapts transport route fields to an internal stamp, maps `elicitation.*` to
  `consent.*`, and excludes `elicitation_unavailable`, which cingulate never emits.
- `consent.rs` pins cingulate's additive fields and the console refusal without
  removing anything from the parsed reply or pointer.
- `consent_outcomes_refs.rs` regenerates core outcome vectors through the real
  handler, including requester correlation, single-use grants, and settlement-time
  detach status.
- `pins.rs` pins each SHA256SUMS and verifies complete file coverage and bytes.
- `contracts.rs` checks the two contract corpora, `danger-option-group-v1` and
  `permissions-v1`, which are written from `docs/contracts/` before any
  implementation exists: each `.jcs` against an independent RFC 8785
  canonicalizer, each `.sha256`, and the contracts' cross-case rules.

`examples/generate_vectors.rs` writes those two corpora. Check them from the
workspace root with
`cargo run -p cingulate-vectors --example generate_vectors --locked -- --check`;
run it without `--check` to rewrite them after a deliberate contract change,
then update the manifest pins in `pins.rs`.
The core outcomes vectors use
`cargo run -p cingulate-vectors --example generate_consent_outcomes_refs_vectors --locked -- --check`.

Comparisons remove only `requesterLabel` and `atConsole` from the vectors' phone
projections, or `requester_label` from their requester records. Opaque nested
JSON is never filtered. Placeholder substitution preserves numeric clock values
as JSON integers. No test regenerates fixtures from the implementation.
