# Consent card text v1 served vectors

This corpus pins card-text requests and replies produced by Cingulate's real
in-process handler. Every store is a fresh temporary SQLite store with a fixed
clock and the sweep timer disabled. Successful request and readback files are
untouched JSON bytes from the handler, including the `result` envelope and no
trailing newline. Refusal reply files serialize the actual handler error code and
message as JSON.

All inputs here are invented test data: agents, sessions, cards, devices, ids
and times name no real person, device or deployment. Principals such as
`reserved:callosum` are protocol identifiers, not accounts.

Both requests carry `facts`, the card's label-and-value detail rows, written as
plain sentences rather than JSON, with labels such as `Runs`, `Can wake` and
`Token limit`. The `flow_install` dry run has an
empty `problems` list; the package dry run has one problem. Both cards include
`source` and exercise requester and phone readbacks. The package list projection
keeps these text fields while omitting the large package script and manifest.

| File | Contents |
|---|---|
| `flow-install-request.json`, `flow-install-request-reply.json` | Request envelope and the real creation reply. |
| `flow-install-list-pending.json`, `flow-install-phone-get.json` | Phone list and get projections for the pending installation card. |
| `flow-install-requester-get.json` | Requester get projection using snake_case field names. |
| `flow-install-answer.json` | Phone answer readback, including the served card. |
| `package-request.json`, `package-request-reply.json` | Request envelope and the real creation reply. |
| `package-list-pending.json`, `package-phone-get.json` | Phone list and get projections for the pending package card. |
| `package-requester-get.json` | Requester get projection using snake_case field names. |
| `package-answer.json` | Phone answer readback, including the served card. |
| `dry-run-text-on-consent-request.json`, `dry-run-text-on-consent-reply.json` | `elicitation_invalid_request` refusal when a consent card carries `dry_run_text`. |
| `would-over-cap-request.json`, `would-over-cap-reply.json` | `elicitation_invalid_request` refusal when `would` has more than eight items. |
| `SHA256SUMS` | SHA-256 hashes of all corpus files except this manifest. |
| `README.md` | This index and reproduction instructions. |

From the workspace root:

```sh
cargo run -p cingulate-vectors --example generate_consent_card_text_vectors --locked -- --check
cargo test -p cingulate-vectors --test consent_card_text --locked
```

The generator's `--check` mode compares every generated file byte-for-byte and
checks exact directory coverage without writing. Running it without `--check`
rewrites this corpus and its `SHA256SUMS`; it does not touch any other corpus.
