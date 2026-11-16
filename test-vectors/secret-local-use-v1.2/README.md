# The vault's local-use samples v1.2

`vectors.json` is copied byte-for-byte from the vault's local-use samples v1.2
(`secret-local-use-vectors-v1.2`). All keys and secret material are fixed, public
**test-only** values; never use them in a real vault.

## Pinned bytes

- `vectors.json` SHA-256:
  `02b6c9a5cff3f67434e9d74a6d510ca9ee81c7118002b3767dfe6250359f1e18`
- `SHA256SUMS` SHA-256:
  `6d5d00f897425709b1796813ae1d40b654da2092c40152e803d79e3c7ddb0b56`

The copied `SHA256SUMS` covers only `vectors.json`, not itself or this consumer
README. Verify the corpus from this directory with `sha256sum -c SHA256SUMS`.
The tests also pin both file hashes and check the decoded byte records and
approval bindings; they do not regenerate the signed statement bytes.

## Changes from v1.1

SSH requests (`secret.use` with a local target and `secret.ssh_sign`) and SSH
statements no longer carry `purpose`. An SSH request carrying that member, even
an empty string, is refused. SSH approvals bind the updated statement bytes and
digests. The statement version stays `ck-secret-local-use-statement/v1`.

Every sudo case is byte-identical to v1.1. Sudo statements still require a
non-empty `purpose`. The local and forwarded cases, public keys and userauth data
remain in the pack. The historical v1 and v1.1 corpora are retained; their SSH
statements with `purpose` are now tested as refusals rather than approvals.
