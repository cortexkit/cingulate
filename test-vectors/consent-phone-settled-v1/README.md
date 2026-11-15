# Settled consent phone reads after acknowledgment, v1

These vectors submit the package request from `consent-package-card-v1` as
`reserved:prefrontal-core` through Cingulate's real handler, settle each card,
read `consent.answers` and acknowledge its cursor as core, then capture the phone
`consent.get` result. Each scenario uses fresh scratch storage and the fixed clock
from the vector fixture. The generator captures raw success replies; the pruned
case records the handler's `elicitation_not_found` refusal as `{code, message}`.

All inputs here are invented test data: agents, sessions, cards, devices, ids
and times name no real person, device or deployment. Principals such as
`reserved:callosum` are protocol identifiers, not accounts.

The answered card with no reported execution shows that the selected choice and
both option labels remain available while execution is pending. The completed
card reports `executed`. The not-run case is also supported for a core-raised
card: the requester may report `approved_not_executed`, so that case is included.
Expired and withdrawn cards remain readable after acknowledgment. Once the
answered card reaches the 24-hour retention boundary, the handler prunes its
payload and phone get returns `elicitation_not_found`. The acknowledged answer
attempt uses a different option and returns `conflict` with the committed card.

| File | What it shows |
|---|---|
| `request.json`, `request-reply.json` | The unchanged package request body and the actual core handler response. |
| `answered-after-ack-pending-execution-phone-get.json` | An acknowledged answer remains readable while no execution outcome is reported. |
| `answered-after-ack-executed-phone-get.json` | The phone projection retains the `executed` outcome. |
| `answered-after-ack-not-run-phone-get.json` | The phone projection retains the `approved_not_executed` outcome. |
| `expired-after-ack-phone-get.json`, `withdrawn-after-ack-phone-get.json` | Terminal cards remain readable after acknowledgment. |
| `pruned-after-ack-phone-get.json` | The card is not found at the 24-hour retention boundary. |
| `answer-acknowledged-conflict.json` | A settled acknowledged card still refuses a different answer with `conflict` and the card. |
| `SHA256SUMS` | SHA-256 hashes of every other file, including this README. |

From the workspace root:

```sh
cargo run -p cingulate-vectors --example generate_consent_phone_settled_vectors --locked -- --check
cargo test -p cingulate-vectors --test consent_phone_settled --locked
```

The generator compares every captured reply, this README, and `SHA256SUMS`
byte-for-byte, and checks exact directory coverage. Running without `--check`
writes only this vector set. The integration test also pins the answered phone
projection and the manifest is pinned in `tests/pins.rs`.
