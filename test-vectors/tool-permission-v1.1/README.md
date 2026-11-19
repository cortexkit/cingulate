# Served permission contract corpus, v1.1

This corpus pins the [requester v2](../../docs/contracts/consent-requester-v2.md)
and [phone v2](../../docs/contracts/consent-phone-v2.md) contracts. It is a
complete revision of [v1](../tool-permission-v1/README.md), which is unchanged.
Each `*-request.json` is a framed request; its `*-reply.json` is untouched real
handler response bytes. `bind-stamps.json` supplies the trusted fixture identity.
The clock is fixed at 1000 ms and every generation uses a fresh scratch store.
The macOS fixture explicitly models macOS even on Linux.

All inputs are invented test data: agents, sessions, cards, devices, ids and
times name no real person, device or deployment. Principals such as
`reserved:callosum` are protocol identifiers, not accounts.

Tool permission cards now use the stamped scope kind for the short title and the
asking agent's display name for the headline. `named-head-*` shows a head's read
request with a resolvable name; the resolver fixture maps the stamped
`agent-example` id to `Navigator`. The existing worker and operator-app
scenarios have no resolvable asker name and use the neutral headline fallback.

Other scenarios cover a worker opening and head-leg get; refused phone head-leg
answer; unheld head grant refusal; escalation with a note; phone get and pending
list; user scope grant applying to the worker; losing late head answer;
persistent call-key replay; read-only settled await; a head's own user ask and
immediate pending await; withdrawal; informational macOS opening, repeat and
phone get; a worker ask refused to an unrelated withdrawer and then withdrawn
by its scope owner; and withdrawal of an already-answered ask, returned
unchanged. `withdraw-bind-stamps.json` supplies the withdrawal callers.
The `notice-*.json` files are actual durable outbox bodies.

`operator-app-*` shows a desktop user-leg answer. `operator-app-peer.json`
supplies the trusted bind peer stamp; `operator-app-audit.json` is the persisted
audit row. The attempt records `via: "operator_app"` while keeping
`surface: "user"` and final `answered_by: "user"`; it has no phone device
fingerprint.

Regenerate locally (this command writes source-controlled vectors):

```
cargo run -p cingulate-vectors --example generate_access_vectors --locked
```

Check without writes:

```
cargo run -p cingulate-vectors --example generate_access_vectors --locked -- --check
cargo test -p cingulate-vectors --test access --locked
```

`SHA256SUMS` covers every file except itself. Tests check exact file coverage and
hashes, pin both versions' manifest digests, and regenerate the current served
bytes. For future wire changes, create a new corpus and pin its manifest rather
than editing a published corpus.
