# Local secret cards served v1.2

These invented local-secret requests pin the consent cards served to desktop and
phone operator surfaces. All scopes, accounts, keys and credentials are fake. The
vault statements are sample data, not usable authorizations, and the sealed-text
recipient key is fake.

The seven `*-request.json` files preserve AFT's local secret request cards v1.2
byte for byte. The vault cards carry the exact local statements from the vault's
local-use samples v1.2, vendored in `test-vectors/secret-local-use-v1.2`, without
rewriting their bytes or expiry times. SSH cards omit `purpose` and the
`The agent says` fact; sudo retains its purpose. Only the SSH request and served
files differ from v1.1. The sudo and Git files remain byte-identical, and both
historical served corpora and their manifest pins remain unchanged.

The input test reconstructs AFT's `SHA256SUMS` using the original filenames and
card order, and pins its SHA-256:
`a826415ddc9274000272defc69c485954214df9844783106e5b05e9dee46524e`.
The vault statement bytes and request ids are also compared directly with the
vendored local-use samples v1.2.

Each request is filed through the real handler as `reserved:aft`. The five
ordinary cards use the v1 route-stamp construction: a daemon-stamped worker scope
`fake-agent` at epoch 7, owned by `reserved:prefrontal-core`, with parent
`sc_head_alf` at epoch 7 and no stamped agent id. The worker fixture resolves
that live parent to `agent-alf`, and the resolver labels their subject `ALF`.

The two vault cards instead use the same `secret_use::filer` construction as
Cingulate's local-use tests. Both routes have a daemon-stamped head scope `H` at
epoch 7, owned by `reserved:prefrontal-core`, with no parent and owner
authorization set. The `sudo-vault` route stamps agent id `agent-A`; the
`ssh-vault` route has no stamped agent id, matching its statement's explicit
`agent_id: null`. These routes match the scope and agent attribution bound into
each statement. Filing the statements under the ordinary worker stamp would be
refused, not treated as permission to use a secret. All trusted identities are
fixed independently of request params, not obtained from their contents.
The vault fixture uses the existing unavailable resolver, so it does not invent
a display name for the stamped agent.

Each case has a fresh temporary store, card id `el_1`, a fixed filing clock of
1700000000000 milliseconds and change-log instance
`0123456789abcdef0123456789abcdef`. The vault statements expire at
1700000600000 milliseconds, so the filing clock precedes their expiry.

| Prefix | Request |
|---|---|
| `sudo-typed` | Type a sudo password into a sealed-text input; Send once or decline. |
| `sudo-vault` | Request one local sudo password use from the vault. |
| `ssh-agent` | Approve SSH key use once, for an hour or for the session. |
| `ssh-vault` | Request local SSH signing from the vault with the same durations. |
| `ssh-passphrase` | Type a key passphrase into a sealed-text input; Send once or decline. |
| `git-push` | Approve saved credentials for a Git push. |
| `git-fetch` | Approve saved credentials for a Git fetch. |

For each prefix, the following files pin the exact handler response or stream
item bytes, without parsing and reserializing the output:

| Suffix | Contents |
|---|---|
| `-request.json` | The input `consent.request` params, not a transport envelope. |
| `-list-pending.json` | The full phone `consent.list_pending` response envelope, containing the single pending row. |
| `-get-pending.json` | The phone `consent.get` response envelope containing the operator surface record. |
| `-watch-created.json` | The phone `consent.watch_pending` item pushed when that request is filed. |

The five ordinary-card prefixes (`sudo-typed`, `ssh-agent`, `ssh-passphrase`,
`git-push` and `git-fetch`) also have `-desktop-list-pending.json` and
`-desktop-get-pending.json` files. These contain the exact local desktop response
envelopes for the same operations. There are 31 served files and seven inputs.

The paired-phone route uses the fixed fake fingerprint
`11111111111111111111111111111111`. All three operations declare
`clientFeatures: ["secret-use-approval"]`. The verified local desktop operator
route uses the fake daemon-stamped app identity `sample-desktop` at version 5.
For ordinary cards, the phone and desktop list rows and get responses match, but
their list revisions differ: the phone revision has a `:secret-use` suffix.
The generator pins each surface's list and get envelopes separately and asserts
that their ordinary-card watch items are byte-identical.
The two vault cards are phone-only under Cingulate's existing visibility policy:
the phone fingerprint and feature declaration are required to read them.
The live watches start before filing; their initial `{"type":"reread"}` items
are checked but are not card-specific samples.
Unary responses retain their `result` envelope; watch items have their own stream
shape. JSON served files have no trailing newline. Inputs retain their original
whitespace. Every card defaults to decline. Vault cards retain Cingulate's generic
Allow/Decline chrome and carry the statements intact for detailed phone rendering.

`README.md` describes the corpus. `SHA256SUMS` covers every file except itself,
in filename order, and the manifest digest is pinned independently of v1 and v1.1
in the test. Do not edit served records by hand.

Check regeneration and all pinned manifests:

```sh
cargo test --locked -p cingulate-vectors --test local_secret_cards
```

Regenerate v1.2 through the full generator after an intentional change:

```sh
cargo run --locked -p cingulate-vectors --example generate_local_secret_cards_vectors -- local-secret-cards-served-v1.2
```

Review the served changes and update only the v1.2 manifest pin in the test.
Append `--check` to compare every file without rewriting it. Omitting the corpus
name preserves the generator's original v1 invocation; historical v1 and v1.1
SSH vault inputs are now refused because they carry `purpose`.
