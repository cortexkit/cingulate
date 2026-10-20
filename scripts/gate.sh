#!/usr/bin/env bash
# The checks a change must pass before it lands on main. This repository is
# private, and GitHub Actions does not run for private repositories on this
# account, so the gate runs here (or on a remote runner) instead of in CI.
# Run it from the repository root; any failing step stops it with that
# step's exit code.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

step() { printf '\n== %s\n' "$*"; }

step "format"
cargo fmt --all -- --check
step "clippy"
cargo clippy --workspace --all-targets --locked -- -D warnings
step "tests"
cargo test --workspace --locked
step "vector corpora match their generators"
cargo run -q -p cingulate-vectors --example generate_vectors --locked -- --check
cargo run -q -p cingulate-vectors --example generate_consent_outcomes_refs_vectors --locked -- --check
cargo run -q -p cingulate-vectors --example generate_no_expiry_vectors --locked -- --check
cargo run -q -p cingulate-vectors --example generate_session_ended_vectors --locked -- --check
cargo run -q -p cingulate-vectors --example generate_watch_pending_vectors --locked -- --check
step "gate passed"
