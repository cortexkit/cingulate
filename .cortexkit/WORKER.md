# Worker guide

Run every command from the worktree root. Serialize cargo invocations: one cargo command at a time, never in parallel, because other workers share this machine. Run builds and tests in the foreground and wait for them; never poll a background job with sleep loops.

## Gates

- Format: `cargo fmt --all -- --check`. Counts as ran when it exits 0 with no diff printed. It is silent on success, so report `cargo fmt --version` with it.
- Lint: `cargo clippy --workspace --all-targets --locked -- -D warnings`. Counts as ran when it exits 0, the last line is `Finished ...`, and there are no `warning:` or `error:` lines.
- Tests: `cargo test --workspace --all-targets --locked`. Counts as ran when every `test result:` line reads `ok` and the passed counts sum to more than 0.
- Independence: `cargo tree --workspace --edges normal` must list no `prefrontal-*` crate. Cingulate never depends on an orchestrator.

## Rules

- Every card defaults to a decline. Never add a path where silence, a timeout or an error grants anything.
- Requester and subject identity come only from the subc daemon's route stamp, never from a request body.
- Tests use scratch stores under a temp directory, never `~/.local/share/cortexkit/`.

## Commands to avoid

- Bare package runners (`npx <tool>`, `bunx <tool>`): they can fetch an unrelated package of that name, print nothing and exit 0.
- `rm -rf target/`: use `cargo sweep --time 7` to reclaim space.
- `git push` from a worker worktree: the chair merges and pushes.
