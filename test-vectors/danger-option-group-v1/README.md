# Danger option group v1

Wire vectors for the danger option group on `consent/v1` cards. The normative
contract is `docs/contracts/danger-option-group.md`. These vectors are written
from that contract, not captured from an implementation: no cingulate release
serves the `group` field yet, and an implementation must match these bytes, not
regenerate them.

## Encoding

Each case has three files:

- `<case>.json`: pretty JSON (`serde_json` pretty printer, sorted keys, two-space
  indent) with one trailing newline. For reading.
- `<case>.jcs`: the same value in RFC 8785 canonical form, no trailing newline.
  This is the pinned form.
- `<case>.sha256`: the lowercase 64-character hex SHA-256 of the `.jcs` bytes, no
  trailing newline.

The corpus holds ASCII strings and integers only. `SHA256SUMS` covers every file
except itself, including this README. Verify from this directory with
`shasum -a 256 -c SHA256SUMS`. `crates/cingulate-vectors/tests/pins.rs` pins the
SHA-256 of `SHA256SUMS`.

Regenerate or check from the workspace root:

```sh
cargo run -p cingulate-vectors --example generate_vectors --locked
cargo run -p cingulate-vectors --example generate_vectors --locked -- --check
```

`crates/cingulate-vectors/tests/contracts.rs` re-derives every `.jcs` with an
independent canonicalizer, checks every `.sha256`, and checks the contract's
invariants across cases (stripping, the filing rules, the answer refusal).

## Case fields

- `tag`: `danger-option-group-v1`.
- `method`: the `consent/v1` operation. Surfaces reach the same operations as
  `elicitation.*` through core's legacy relay until the cutover.
- `route`: test notation for the daemon's route stamp, never a request field.
  `principal` is the attested principal; `carried_agent_id` is the agent a
  carrier route was opened for; `operator_surface: true` marks an operator
  surface stamp (the paired phone through Callosum).
- `request`: the operation params exactly as sent.
- `reply`: the success value (the `result` of the reply envelope), or
- `error`: the error envelope `{code, message}`. Clients branch on `code`;
  messages are illustrative.

## Scenario

Cerebellum, carrying ALF (`agent-alf`), files its app card for TextEdit at
`createdAtMs` 1000000 as `el_1`. The card has five options: `app_30m`,
`app_always`, `deny` (the default, a decline) and, in the danger group,
`computer_30m` and `computer_always`.

| Case | What it pins |
|---|---|
| `file-card` | `consent.request` with danger options, accepted as `el_1`. |
| `list-pending-declared` | `consent.list_pending` declaring `option-group`: all five options. |
| `list-pending-undeclared` | The same read without the feature: the two danger options are stripped; nothing else changes. |
| `get-declared` | Surface `consent.get` declaring the feature: all five options. |
| `get-undeclared` | Surface `consent.get` without it: three options. |
| `refuse-danger-default` | Filing refusal: the default `deny` is in the danger group. |
| `refuse-only-danger-grants` | Filing refusal: every grant option is a danger option. |
| `refuse-unknown-group` | Filing refusal: `group` is `caution`. The message is serde's wording for this input. |
| `answer-danger-undeclared` | `consent.answer` naming `computer_30m` without the feature: refused by name; nothing changes. |
| `answer-danger-declared` | The same answer declaring the feature: settles the card; the reply's record keeps all options. |

Each refusal is a minimal edit of the `file-card` request. Cases are independent
reads of the one pending card except `answer-danger-declared`, which settles it
at 1005000.
