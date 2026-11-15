# Elicitation records v1

Tag: `elicitation-records-v1`. These are the phone and push record shapes of the
earlier `elicitation.*` operations, which Cingulate's `consent.*` operations
keep. Every agent, card, option and timestamp in them is invented test data.
Ids are symbolic (`el_head`) and the clock starts at 1000.

`crates/cingulate-vectors/tests/records.rs` files the same cards in a scratch
store and requires the stored phone projections to equal these vectors,
normalizing only minted ids and clock timestamps (and removing the
`requesterLabel` and `atConsole` fields, which Cingulate adds).

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

Granting the three phone operations to the phone's routes is deployment
configuration and is not in this repository. A deployment must grant them before
enabling typed cards.

## Integrity manifest

`SHA256SUMS` covers the eight JSON vectors and this README.
`crates/cingulate-vectors/tests/pins.rs` pins the manifest's SHA-256 and checks
that it covers every file.
