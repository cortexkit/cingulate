# Secret-use approvals v1

`secret-approval-v1.json` is copied byte for byte from Claustrum's
`secret-approval-vectors-v1` tag, commit `cefc5c7041ffe93d17c4fcc80e5847260f3ad121`,
at `crates/credentials-core/tests/vectors/secret-approval-v1.json`.
Its SHA-256 is `efd4ba60641586e143bce81087e4f9f5aaf8a99ffede74190303da7d4495cc27`.
These are public deterministic test keys, never production credentials.

`single-phone-get.json` and `batch-phone-get.json` are untouched response bytes
from Cingulate's real `consent.get` path on a stamped phone route advertising
`clientFeatures: ["secret-use-approval"]`. They file the `approval` and
`batch-omitted` statements respectively, at 1700000000000 ms, in fresh scratch
stores. Phone records use `secretUse`; its nested vault items retain snake_case.
The phone renders requested access from the statements, not the generic title.

Regenerate with `cargo run -p cingulate-vectors --example generate_secret_use_vectors --locked`.
Use `--check` to compare without writing. The `secret_use` integration tests
regenerate and compare both served records byte for byte, exercise the three
use-approval cases through filing/answering, and verify checksum coverage.
