# Secret-use approvals v2 (forwarded requests)

When an agent on another paired computer asks to use a secret, the home
computer's vault serves the grant and writes a v2 statement. A v2 statement is
exactly the v1 statement plus one key, `asking_member_id`: `fed-computer:`
followed by 32 lowercase hex characters, naming the computer the agent is on.
`v` is `ck-secret-use-statement/v2`; `member_id` still names the home vault.
Every other key, the canonical JSON bytes and the envelope's `statement_sha256`
binding are the same as v1. Requests made on the home computer stay v1.

Everything here is invented test data. The vault statements, keys and
signatures are Claustrum's published test values, and every agent, device,
secret name and id names no real person, device or account.

`secret-approval-v2.json` is copied byte for byte from Claustrum commit
`af71c1648da4aeccd624e84894c4a6671c78b676`, at
`crates/credentials-core/tests/vectors/secret-approval-v2.json`.
Its SHA-256 is `6117ab23408df9aa3ae901ddab58b8ea395ba310e350f4ec8b4e5e9ed338c26a`.
It holds two cases, `forwarded-use-api-key` and `forwarded-use-site-login`.
These are public test values, never production credentials.

Every other file is produced by Cingulate's real handler in fresh scratch
stores at 1700000000000 ms. The filer is `reserved:aft` scoped to `agent-A`;
the phone is the callosum-stamped `fed-phone:11111111111111111111111111111111`
declaring `clientFeatures: ["secret-use-approval"]`. The allow envelope is
built by the test, not signed: Cingulate checks its shape, the answering
phone's fingerprint and each statement's digest, while the vault verifies the
signature. Its digests equal the vault's `statement_sha256` values.

| File | What it shows |
|---|---|
| `secret-approval-v2.json` | Vault vectors copied byte for byte. |
| `single-phone-get.json` | Phone `consent.get` response bytes for the `forwarded-use-api-key` card. |
| `batch-phone-get.json` | Phone `consent.get` response bytes for a card holding both cases. |
| `filer-request.json` | Filer `consent.request` for both cases, with its reply. |
| `phone-allow.json` | Phone `consent.answer` allowing both statements for `hour`, with its reply. |
| `filer-await-allowed.json` | Filer `consent.await` after that allow, with `secret_use_answer`. |
| `filer-await-declined.json` | Filer `consent.await` after a decline, every item declined. |
| `filer-refusals.json` | Named filings refused with `elicitation_invalid_request` and nothing stored. |

The refusals change only what their name says, on every statement unless the
name is about mixing: a v1 `v` carrying `asking_member_id` (a string or
`null`), a v2 statement without it or with `null`, a malformed asker
(uppercase, short, long, `fed-phone:`, bare hex, a number), an asker equal to
`member_id`, a flow statement version on a secret_use card, two different
askers on one card, and a v1 statement next to a v2 one.

Regenerate both v2 corpora with
`cargo run -p cingulate-vectors --example generate_forwarded_secrets_vectors --locked`.
Pass `-- --check` to compare without writing. The `forwarded_secrets`
integration tests regenerate every generated file, compare it byte for byte
and verify `SHA256SUMS` coverage.
