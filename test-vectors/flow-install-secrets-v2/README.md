# Flow-install secret approvals v2 (forwarded requests)

When a flow installed on another paired computer needs a secret, the home
computer's vault serves the grant and writes a v2 flow statement: exactly the
v1 statement plus `asking_member_id`, `fed-computer:` followed by 32 lowercase
hex characters, naming the computer the flow is on. `v` is
`ck-secret-flow-statement/v2`; `member_id` still names the home vault. Every
other key, the canonical JSON bytes and the envelope's `statement_sha256`
binding are the same as v1. Requests made on the home computer stay v1.

Everything here is invented test data. The vault statements, keys and
signatures are Claustrum's published test values, and every agent, device,
secret name and id names no real person, device or account.

`secret-flow-approval-v2.json` is copied byte for byte from Claustrum commit
`af71c1648da4aeccd624e84894c4a6671c78b676`, at
`crates/credentials-core/tests/vectors/secret-flow-approval-v2.json`.
Its SHA-256 is `05ec7aef2351f58e7e4151526425c5be70d678bfba85931ccddd0f4de246551c`.
It holds two cases, `forwarded-flow-api-key` and `forwarded-flow-site-login`.
These are public test values, never production credentials.

Every other file is produced by Cingulate's real handler in fresh scratch
stores at 1700000000000 ms. The filer is `reserved:prefrontal-core`; the phone
is the callosum-stamped `fed-phone:11111111111111111111111111111111` declaring
`clientFeatures: ["secret-use-approval"]`. The install card is the one in
`flow-install-secrets-v1` with `flow_identity` set to the vault statements'
shared flow and `secrets` holding both vault statements unchanged. The allow
envelope is built by the test and not signed; its digests equal the vault's
`statement_sha256` values.

| File | What it shows |
|---|---|
| `secret-flow-approval-v2.json` | Vault vectors copied byte for byte. |
| `filing.json` | Core's `consent.request` for the two-secret install card, with its reply. |
| `phone-get.json` | The phone's `consent.get` projection, nested vault bytes unchanged. |
| `allow.json` | The phone's install answer with its `version` envelope, and the reply. |
| `decline.json` | A decline, which needs no envelope, and the reply. |
| `filing-refusals.json` | Named filings refused with `elicitation_invalid_flow_secrets` and nothing stored. |

The refusals are the same named changes as in `secret-use-approval-v2`, with a
secret_use statement version on the flow card as the wrong-family case.

Regenerate both v2 corpora with
`cargo run -p cingulate-vectors --example generate_forwarded_secrets_vectors --locked`.
Pass `-- --check` to compare without writing. The `forwarded_secrets`
integration tests regenerate every generated file, compare it byte for byte
and verify `SHA256SUMS` coverage.
