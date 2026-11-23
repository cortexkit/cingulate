#!/usr/bin/env bash
# The checks a change must pass before it lands on main. CI runs the same checks
# on Linux, macOS and Windows (.github/workflows/ci.yml); this script runs them on
# the current machine. Run it from anywhere in the repository; any failing step
# stops it with that step's exit code.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

step() { printf '\n== %s\n' "$*"; }

dev_feature="ck-cingulate/dev-local-operator"

step "format"
cargo fmt --all -- --check
step "clippy (default features)"
cargo clippy --workspace --all-targets --locked -- -D warnings
step "clippy ($dev_feature)"
cargo clippy --workspace --all-targets --locked --features "$dev_feature" -- -D warnings
step "tests (default features)"
cargo test --workspace --all-targets --locked
step "tests ($dev_feature)"
cargo test --workspace --all-targets --locked --features "$dev_feature"
step "vector corpora match their generators"
for example in \
  generate_vectors \
  generate_broker_action_vectors \
  generate_consent_outcomes_refs_vectors \
  generate_no_expiry_vectors \
  generate_sandbox_escape_vectors \
  generate_session_ended_vectors \
  generate_watch_pending_vectors; do
  cargo run -q -p cingulate-vectors --example "$example" --locked -- --check
done
step "no orchestrator dependency"
# Capture first, so a failing cargo tree stops the gate instead of reading as "no match".
tree="$(cargo tree --workspace --edges normal --locked --prefix none --color never)"
test -n "$tree"
if printf '%s\n' "$tree" | grep -E '^prefrontal-'; then
  echo "an orchestrator crate is a normal dependency" >&2
  exit 1
fi
step "gate passed"
