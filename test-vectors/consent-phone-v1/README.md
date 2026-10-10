# Phone consent responses, v1

These files are the untouched JSON response bytes from the consent handler,
including the `result` envelope, with no appended newline. They are not authored
examples or normalized projections. The phone route is bound as
`reserved:callosum` with harness `fed-phone:vector-device`. Stores are temporary,
the clock starts at 10,000 ms, and fresh-store ids start at `el_1`.

The package request comes from the existing `consent-package-card-v1` request
fixture, unchanged. Answer and withdrawal happen at 11,000 ms; expiry happens at
the request's exact deadline. The other three requests are defined in the shared
generator input and are submitted through `consent.request`, not inserted as
records. Browser subject identity comes from the requester identity the daemon
attaches to the requester's connection, not from anything in the request body.

| File | Response shown |
|---|---|
| `package-list-pending.json` | Pending package in the first list page; source and manifest bodies omitted; revision included. |
| `package-get-pending.json` | Full pending package fetched by id. |
| `package-get-answered.json` | Full package after selecting `allow`. |
| `package-get-expired.json` | Full package expired at its deadline, with no selected choice. |
| `package-get-withdrawn.json` | Full package withdrawn by its requester, with no selected choice. |
| `answer-success.json` | First successful answer of the package. |
| `answer-already-answered.json` | Same choice repeated, with `alreadyAnswered: true`. |
| `answer-different-choice.json` | Different choice after answer: conflict with the committed card. |
| `answer-expired.json` | Answer at expiry: conflict with the expired card. |
| `answer-withdrawn.json` | Answer after withdrawal: conflict with the withdrawn card. |
| `answer-not-found.json` | Unknown id: `ok: false`, `code: not_found`, no card. |
| `list-unchanged.json` | Matching conditional revision: no records or continuation. |
| `flow-install-get-pending.json` | Pending installation card raised by `reserved:prefrontal-core`. |
| `flow-decision-get-pending.json` | Pending decision card raised by `reserved:basal`. |
| `browser-get-pending.json` | Pending browser consent raised by `reserved:cerebellum`, including facts and preview. |
| `other-list-pending.json` | All three non-package kinds listed in expiry/id order. |
| `SHA256SUMS` | SHA-256 hashes of every JSON file and this README. |
| `README.md` | This index and reproduction instructions. |

From the workspace root:

```sh
cargo run -p cingulate-vectors --example generate_phone_vectors --locked -- --check
cargo test -p cingulate-vectors --test phone --locked
```

With `--check`, the generator regenerates vector bytes and `SHA256SUMS` in memory
and fails on any byte difference or file-coverage mismatch, without writing
files. Running without `--check` rewrites this corpus's files, including
`SHA256SUMS`; it does not write any other corpus. Do that only for an intended
change to what the module returns. After such a change, update `SHA256SUMS`, its
pinned manifest hash in the integrity test, and the corpus version tag. The
regeneration test compares every response byte; the integrity test checks exact
directory coverage, every hash, and a pinned hash of `SHA256SUMS` itself.

The app-facing contract is [consent-phone-v1](../../docs/contracts/consent-phone-v1.md).
