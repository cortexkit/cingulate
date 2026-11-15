# Extended secret statement samples

`secret-approval-v3.json` and `secret-flow-approval-v3.json` are byte-for-byte
copies from claustrum tag `secret-multi-host-vectors-v1` (commit `cc7544d7`), under
`crates/credentials-core/tests/vectors/multi-host/`. The two entries in
`SHA256SUMS` are copied from that directory's authority manifest. Do not regenerate
or edit these samples in Cingulate; their sole writer is claustrum's
`scripts/secret-multi-host-vectors.py`. Changes require the claustrum repository
owner's sign-off, as specified by that tag's `docs/wire-contract-v1.md`, section
"Conformance corpus and its single writer".

Everything here is invented test data. The vault statements, keys and
signatures are Claustrum's published test values, and every agent, device,
secret name and id names no real person, device or account.

`crates/cingulate-vectors/tests/multi_host_secrets.rs` pins both file digests and
every statement's canonical JSON (JCS) bytes and digest. All eight
`approval_required` cases per file decode, file, project to the phone unchanged
and bind an approval envelope:
local and forwarded eight-rule API keys, lone wildcards without prefixes, two
login origins and four login origins.

The files also include vault-side approval refusals for pending-asker mismatches
and request refusals for reserved flow ids. Cingulate cannot inspect pending vault
rows, so those approval outcomes remain vault tests, not Cingulate decisions.
Cingulate tests its own closed shapes, version and v3 kind agreement, asker
grammar and self-asker exclusion, and the version/forwarding-specific reserved
flow-id rules with independent statement mutations.

Cingulate does not render statement scopes into text or labels. It passes the
complete canonical statement to the phone, whose rendering contract requires
honest multi-entry and wildcard presentation. Existing v1/v2 corpora and fixed
card wording are unchanged.
