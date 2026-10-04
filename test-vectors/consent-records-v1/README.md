# Consent records v1

Tag: `consent-records-v1`. These vectors pin cingulate's additive wire fields.
Op files contain `tag`, `method`, `request`, and `reply` or `error`; push files
contain `tag` and `reply`. They are operation payloads, not result envelopes.
Tests compare parsed JSON after placeholder substitution, with nothing removed.

## Scenario steps

All stores are fresh scratch stores, the sweep timer is stopped, and the clock
is injected. The request body is `../elicitation-request-v1/valid/app-grant-once.json`.
The requester stamp is `direct` with bind session `provider-session`. The
operator stamp is `reserved:callosum`, operator surface true, with no answering
console machine id. No identity is taken from request bodies.

1. **Request and settle:** request with added `binding: {"k":1}`, then answer
   `allow_once` as the operator. `requester-record.json` is the owner's subsequent
   `consent.get` reply for the answered card (before ack or execution reporting).
2. **Console pending:** in another fresh store, HELLO_ACK carries `machine_id: m1`.
   Request with added `at_console: m1`. `push-new-machine.json` is the delivery
   pointer recorded before request returns. `phone-pending-at-console.json` is
   the operator's `consent.get` reply while the card remains pending.
3. **Console refusal:** `answer-not-at-console.json` is the operator's answer of
   `allow_once` to the pending console card. It returns `user_not_at_console`
   because the stamp has no console machine id; no row or notification changes.
4. **Withdraw:** the owner withdraws that same pending console card at the injected
   settlement time. `push-retraction-machine.json` is the recorded retraction
   before withdraw returns. Both console pointers carry `machineId: m1`.

Placeholders follow requester-v1: `@ELICITATION_ID@` is the card in the relevant
scenario; `@NOW_MS@` is its creation time, `@EXPIRES_AT_MS@` is creation + 60000,
and `@ANSWERED_AT_MS@` is settlement time. Clock placeholders become JSON integers.
`@HEAD_AGENT_ID@` would mean `agent-alf`; these direct cards have no agent ids.

`SHA256SUMS` covers exactly these five JSON files and this README. The consuming
test pins the SHA-256 of that manifest. Verify from this directory with
`shasum -a 256 -c SHA256SUMS`. Fixtures are not rewritten during tests.
