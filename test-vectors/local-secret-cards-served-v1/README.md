# Local secret cards served v1

These invented local-secret requests pin the consent cards served to desktop and
phone operator surfaces. All scopes, accounts, keys and credentials are fake. The
vault statements are sample data, not usable authorizations, and the sealed-text
recipient key is fake.

The seven `*-request.json` files preserve the supplied v1.1 `consent.request`
params byte for byte. Each request is filed through the real handler as
`reserved:aft`, using the same route-stamp construction as the other served vectors.
The daemon-stamped worker scope is `fake-agent` at epoch 7, owned by
`reserved:prefrontal-core`, with parent `sc_head_alf` at epoch 7. The existing
worker fixture resolves that live parent to `agent-alf` for ordinary cards and
the resolver labels their subject `ALF`. The vault cards retain the requester's
subject because the worker route has no stamped agent id. These trusted values
are fixed independently of request params.
Each case has a fresh temporary store, card id `el_1`, a fixed filing clock of
1800000000000 milliseconds and change-log instance
`0123456789abcdef0123456789abcdef`. The vault statements expire at
1800000600000 milliseconds. Identity comes from the route stamp, not the params.

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
in filename order, and the manifest digest is pinned in the test.

The two sudo inputs may be revised if the requester changes the command it shows.
Regenerating the served records from the inputs is the vector test's job; do not
edit served records by hand.

Check regeneration and the pinned manifest:

```sh
cargo test --locked -p cingulate-vectors --test local_secret_cards
```

After an intentional input or handler change, regenerate with:

```sh
cargo run --locked -p cingulate-vectors --example generate_local_secret_cards_vectors
```

Review the served changes and update the manifest pin in the test. The same
generator accepts `--check` to compare every file without rewriting it.
