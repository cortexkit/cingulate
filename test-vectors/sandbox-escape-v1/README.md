# Sandbox-escape approval vectors, v1

All agents, devices and paths are invented. Each case records the management
method, params and the reply served by Cingulate. The identities come from
route stamps, not request parameters. See the
[requester contract](../../docs/contracts/consent-requester-v2.md#sandbox-escape-requester-0280)
and the [rendering rules](../../docs/contracts/consent-phone-v2.md#sandbox-escape-approvals-0280).

| File and case | What it shows |
| --- | --- |
| `cases.json`: `file-worker`, `phone-get` | A worker asks the person directly; the card names its lead agent and preserves a multiline command verbatim. |
| `desktop-get`, `desktop-answer-refused`, `head-answer-refused`, `unsupported-get` | A surface that declares the renderer feature but is not an approving device can read, but cannot answer. The head agent cannot answer. Readers without the feature do not see the card. |
| `run-once`, `replay`, `report-execution` | The phone grants one run. The same call key returns the same card after that answer. The filer records what happened. |
| `decline`, `withdraw`, `answer-at-expiry`, `requester-expired` | No run on decline, withdrawal or expiry. |
| `unknown-key`, `long-command`, `control-character`, `text-direction`, `relative-cwd`, `long-expiry`, `wrong-filer`, `missing-agent-stamp` | Refusals for malformed or unauthorized filings. |
| `development.json`: `desktop-get`, `desktop-run-once` | Development builds allow the local operator to answer. |

Regenerate both files and `SHA256SUMS` from the repository root:

```sh
cargo run -q -p cingulate-vectors --example generate_sandbox_escape_vectors --locked --features ck-cingulate/dev-local-operator
```

Append `-- --check` to compare without writing. With default features the
generator checks the release cases and the checksums; the development build
also regenerates the development cases for comparison.
