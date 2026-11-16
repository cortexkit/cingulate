# Structured flow cards, v1

These public examples show the module-self-diagnosis version 3 flow as an
update and as a new install. The package, manifest and script come from an
actual flow request. The update uses a short question, one illustrative warning
and five comparison rows; the install uses five capability rows. The warning
illustrates a missing event announcement, not a verified statement about a
currently running module. Cingulate displays producer text without interpreting
the manifest or verifying its claims.

The update's `changes` is only the author's note, retained for readers without
structured-member support. Repeated purpose, schedule and operation prose is
removed from preview and details. Both cards still default to decline.

Maple is an invented agent. All requester, phone, desktop and scope identifiers
are fixture identities. The test harness supplies them in the daemon-authenticated
route metadata (the route stamp), not in the request body.
No account credentials or private paths are present. Tests use temporary stores.

For each of `update` and `install`:

- `*-request.json` is the producer's `consent.request` params object.
- `*-requester-get.json` is the requester's `consent.get` response.
- `*-phone-list.json` and `*-phone-get.json` are the phone's pending list and get.
- `*-phone-watch.json` is the phone's changed-card watch item.
- `*-desktop-list.json` and `*-desktop-get.json` are the desktop operator's list
  and get responses. Package lists omit script and manifest bodies, not the new
  card members.

All served JSON is captured byte-for-byte through the real decoder, store and
server handlers with a fixed clock and store instance. Do not edit served bytes
by hand. `SHA256SUMS` covers every file except itself in filename order; its own
digest is pinned in the regeneration test.

Regenerate or check without rewriting:

```sh
cargo run --locked -p cingulate-vectors --example generate_flow_card_structure_vectors
cargo run --locked -p cingulate-vectors --example generate_flow_card_structure_vectors -- --check
cargo test --locked -p cingulate-vectors --test flow_card_structure
```

Contracts: [requester](../../docs/contracts/consent-requester-v1.md#optional-structured-flow-card-content)
and [phone](../../docs/contracts/consent-phone-v2.md#structured-flow-card-content).
