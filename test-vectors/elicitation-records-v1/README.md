# Elicitation records v1

Tag: `elicitation-records-v1`. The producer tests in
`crates/prefrontal-core-module/src/elicitation_tests.rs` pin every JSON file by
SHA-256 and compare the vectors to live operation replies (normalizing only
minted IDs and clock timestamps).

Phone operations (grant to both phone peers through the daemon-attested
Callosum operator route):

- `elicitation.list_pending_for_user {}` → `{records: [...]}`
- `elicitation.get {elicitationId}` → record
- `elicitation.answer {elicitationId, choiceId}` → `{ok: true, alreadyAnswered, elicitation}` on success; `{ok: false, code: "conflict", elicitation}` on conflict; `{ok: false, code: "not_found"}` for an unknown id

The answer is the provider option's `id`, never its display label or `effect`.
An unknown effect is neutral. Same-choice repeats succeed with `alreadyAnswered: true`; a different answer
after settlement returns `ok: false`, `code: "conflict"` and the stored elicitation.
A pending record includes `blockUntilMs` only while a request is awaiting an
answer; the timestamp is core's absolute clock time. Durations may be absent
when the option labels describe different grant lengths.

Requesters on core's route use snake_case parameters and record keys (including
`elicitation_id`, `expires_at_ms`, `answered_choice_id`, and `subject.head_agent_id`):
`elicitation.request`, `elicitation.get`, `elicitation.answers {since}`,
`elicitation.ack {cursor}`, `elicitation.await {elicitation_id, timeout_ms}`,
`elicitation.report_execution {elicitation_id, execution}`, and
`elicitation.withdraw {elicitation_id}`. The allowlisted requester principal is
`reserved:plexus`; the record owner includes its bound session. Attribution refs
are not authority. A requester never answers its own record.

`await` waits at most 120 seconds per call and never beyond the record expiry.
If it returns pending the provider keeps waiting, without returning the approval
step to the agent to re-call. Answer reads replay while unacked; pass the returned
cursor to continue and acknowledge only after durable intake. At 24 hours the
answer feed returns an expired tombstone `{elicitation_id, state: "expired", settled_at}`
rather than the original card. `settled_at` is core epoch milliseconds.

The push plaintext stays inside the existing sealed envelope. A new card carries
`{elicitationId, kind: "elicitation", eventAtMs}`; its silent retraction carries
`{elicitationId, kind: "retraction", eventAtMs}`. As with asks, `kind` identifies
the event, while the id key distinguishes the record type. Lock-screen copy
remains generic.

External phone-grant configuration is not in this repository. The existing
`crates/prefrontal-core-module/tests/it/agent_fleet_overview.rs` avatar vector test
similarly calls out grants to both phone peers as an integration prerequisite.
The rollout must grant the three phone operations before enabling typed cards.

## Vendored integrity manifest

Cingulate generated `SHA256SUMS` at hydration from prefrontal `1d4c9b885`.
The eight JSON vectors are unchanged source bytes. The manifest covers those
files and this README; the consuming test pins the manifest's SHA-256.
