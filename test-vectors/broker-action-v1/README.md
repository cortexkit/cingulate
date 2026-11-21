# Broker-action approval vectors, v1

All agents, devices and repositories are invented. Each case records the
management method and parameters and the real Cingulate handler's reply.
The filer and reader identities are route stamps, not request parameters.
See the [broker-action contract](../../docs/contracts/broker-action-v1.md).

| File and case | What it shows |
| --- | --- |
| `cases.json`: `file-one-leg`, `file-legs` | A scoped `reserved:aft` filing with one request, and a scoped `reserved:plexus` filing with ordered `legs`. The `request_digests` entries show the exact request inputs, including a raw query, no query, an empty body and a percent-encoded path segment. |
| `dedup`, `dedup-after-answer`, `different-owner` | Identical bytes return the same card, even after an answer. Another owner does not obtain it. |
| `unknown-statement-key`, `invalid-encoding`, `noncanonical`, `oversize`, `too-many-fields`, `float`, `context-character`, `expiry-override-refused` | Statement and filing validation. For `oversize`, reconstruct the input by decoding `file-one-leg`'s statement, replacing the value at the specified JSON pointer with the repeated string, then encoding canonical JSON as unpadded base64url. |
| `unauthorized-filer`, `missing-scope`, `mismatched-agent`, `filing-at-expiry` | Filing requires both requester authority and a matching daemon-stamped agent on an unexpired request. |
| `phone-get`, `desktop-get-refused`, `unsupported-get`, `unsupported-answer` | A supported phone reads the card. A release desktop or a reader that lacks the feature cannot read it; an unsupported phone cannot answer it. |
| `allow`, `decline`, `binding-v`, `binding-request_id`, `binding-statement_sha256`, `binding-device_fingerprint` | Allow binds the exact statement and answering device. Decline has no envelope. The mismatches are refused. |
| `requester-allow`, `requester-decline`, `requester-expiry`, `withdraw` | The owner's views of allow, decline, expiry and withdrawal. Expiry has no answer object. |
| `answer-at-expiry`, `answer-after-expiry`, `phone-get-missed`, `phone-get-after-retention` | An answer at or after the deadline is refused; get shows the expired card until the 24-hour retention window closes. |
| `development.json`: `desktop-get`, `desktop-signed-refused`, `desktop-allow` | The development switch admits a stamped local operator, refuses signature members and binds allow to the all-zero fingerprint. |

Regenerate both files and `SHA256SUMS` from the repository root:

```sh
cargo run -q -p cingulate-vectors --example generate_broker_action_vectors --locked --features ck-cingulate/dev-local-operator
```

Append `-- --check` to compare without writing. With default features the
generator checks the release cases and the checksums; the development build
also regenerates the development cases for comparison.
