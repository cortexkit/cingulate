# Consent session-ended execution backstop, v1

Served bytes from the real handler with a scratch store and a fixed clock. A
Cerebellum head-session requester creates a generic consent card at 10000 ms;
the phone answers `allow_once` at 11000 ms. The daemon liveness runtime then
definitively reports that recorded asker scope gone, and the maintenance tick
installs `approved_not_executed` with reason `session_ended`.

`requester-get` and `phone-get` pin the two projections of that backstopped card.
The requester uses `execution_reason`; the phone uses `executionReason`. Neither
projection exposes the private asker scope. No requester execution report is sent.

Regenerate with
`cargo run -p cingulate-vectors --example generate_session_ended_vectors --locked`;
append `-- --check` to verify every byte and SHA256SUMS without writing.
