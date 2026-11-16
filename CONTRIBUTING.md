# Contributing to Cingulate

Cingulate decides when a person has said yes. Changes are reviewed for what they do to that
decision, not only for whether tests pass.

## Agree the design first

Behaviour changes need an issue that a maintainer has labelled `design-approved` before the
pull request can be reviewed or merged. The `Design gate` workflow enforces this: put
`Approved issue: #<number>` (or `Refs #<number>`) in the pull request description. A
maintainer can label a pull request `trivial` instead when there is genuinely no design to
agree, such as a typo fix. Wire changes (a new card kind, a field, a refusal code) also need
their contract in [`docs/contracts/`](docs/contracts) and their samples in
[`test-vectors/`](test-vectors), because the phone, the desktop app and every requester are
built against those.

## Rules every change keeps

- **Every card defaults to a decline.** Never add a path where silence, a timeout or an error
  grants anything.
- **Identity comes only from the subc daemon's route stamp,** never from a request body. That
  holds for the requester, the subject agent and the answering surface.
- **Tests use scratch stores under a temporary directory,** never a real data directory.
- **Cingulate depends on no orchestrator.** `cargo tree --workspace --edges normal` must list
  no `prefrontal-*` crate.
- **Test data is invented.** Vectors and fixtures use made-up agents, devices, keys and ids.
  Never commit a capture from a real person, device or deployment.

## The gate

A change lands only when everything below passes. CI runs it on every pull request and every
push to `main` (`.github/workflows/ci.yml`) on Linux, macOS and Windows; run it locally first.
You need only the stable Rust toolchain with `rustfmt` and `clippy`: every dependency comes
from crates.io, and no sibling checkout is needed.

```sh
./scripts/gate.sh
```

It runs, in order:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --locked --features ck-cingulate/dev-local-operator -- -D warnings
cargo test --workspace --all-targets --locked
cargo test --workspace --all-targets --locked --features ck-cingulate/dev-local-operator
cargo run -q -p cingulate-vectors --example <generator> --locked -- --check   # five generators
cargo tree --workspace --edges normal                                          # no prefrontal-* crate
```

The `dev-local-operator` legs exist so the development-only feature keeps compiling and its
tests keep proving that phone-only cards stay phone-only. Release builds never enable it.

A bug fix ships with a test that fails without the fix. If a change makes an existing test fail,
say in the commit message why the old expectation no longer holds; do not quietly rewrite a test
into its opposite.

## Test vectors

Each directory under `test-vectors/` is a versioned corpus that other components pin. Its
`README.md` says what the files show and how to regenerate or check them, and its `SHA256SUMS`
covers the files (usually the README too). Tests pin the digest of each `SHA256SUMS`.

- Never edit vector bytes by hand. Regenerate them with the corpus's generator, and only for an
  intended change to what the module returns.
- A change to a tagged corpus's bytes is a new corpus version: a new directory or a new tag,
  announced in the changelog, so readers can move deliberately.
- Editing a README inside a summed corpus changes its `SHA256SUMS` line and that manifest's
  pinned digest. Update both in the same commit.

## Versions and the changelog

Each crate keeps its own version. Bump the version of every crate whose behaviour or public API
changes, and record the change under its version in [`docs/CHANGELOG.md`](docs/CHANGELOG.md).
Say whether a store migration runs, what readers of the wire need to do, and which vectors
changed. A release of the `ck-cingulate` binary is a `v<version>` tag on `main` matching
`crates/ck-cingulate/Cargo.toml`; see [README.md](README.md#install).

## Commits

- Author commits with your GitHub no-reply address
  (`<id>+<user>@users.noreply.github.com`), not a personal or work address.
- Keep local paths, machine names and personal details out of tracked files.
- Don't commit logs, build output or generated machine output other than the vectors.
- Keep a safety rule, the code that enforces it and the test that catches its loss in one
  commit.

## Security issues

Do not open a public issue for a vulnerability. Follow [SECURITY.md](SECURITY.md).
