# Flow runner served approval cards, v1.1

This version uses the flow runner's v1.4 filing samples. Compared with
[served v1](../basal-flow-install-cards-served-v1/), only the `flow-secrets`
statements changed: both are now local v1 statements from the same vault, each
with a flat `use` object for a single literal host. The other six filings are
byte-identical.

The seven `*-request.json` files in
`test-vectors/basal-flow-install-cards-served-v1.1/` are the flow runner module's filing samples,
copied byte for byte as `provider`, `method` and `params` envelopes. The generator
files their `params` through Cingulate's real `consent.request` handler on a plain
`reserved:basal` route without an agent scope. Cingulate derives the requester and
subject from the route stamp, not from the author details in the filed payload.
All people, accounts and devices named in the samples are invented. No credential
values are included. Every default choice declines.

| Prefix | Filing |
| --- | --- |
| `agent-new` | An agent's first install of the sample queue flow. |
| `agent-update-v2` | The sample queue flow updated to version 2, with a comparison against version 1. |
| `flow-secrets` | A flow install with two local v1 secret approval statements on one vault. |
| `max-explanation` | A flow explanation with 1,500 four-byte Unicode characters (6,000 UTF-8 bytes). |
| `operator-install` | A flow install identifying the operator as its author. |
| `package-register` | Registration of an instantiable flow template. |
| `switch-refile-agent` | A refile at attempt 5, with an agent ID and name in the author details. |

The generator files the cards in the order above in one temporary SQLite store
with a fixed clock of `1777593540000` ms and IDs `el_1` through `el_7`. It pins
the store instance to `0123456789abcdef0123456789abcdef`.

| File | Contents |
| --- | --- |
| `*-phone-get.json` | Each paired-phone `consent.get` result envelope. |
| `phone-list-pending.json` | The paired-phone `consent.list_pending` page with all seven cards. |
| `*-desktop-get.json` | The verified local desktop `consent.get` result envelopes for the six non-secret cards. |
| `flow-secrets-desktop-get-refusal.json` | The verified local desktop get's typed `elicitation_not_found` refusal, serialized under `error` because the handler returns no JSON result bytes. |
| `desktop-list-pending.json` | The plain local desktop `consent.list_pending` page using development admission, with all seven cards. |
| `SHA256SUMS` | The hashes of every file except itself, in filename order. The test pins the manifest digest. |

The phone uses the fixture fingerprint `11111111111111111111111111111111` and
declares `secret-use-approval`. The verified desktop get uses `sample-desktop`
at version 5 and also declares `secret-use-approval`. Both routes use the
[same operator get projection](../../crates/ck-cingulate/src/lib.rs#L1733-L1742),
so the six ordinary get responses are byte-identical. In a release build the
local desktop route omits `flow-secrets` from its page and refuses a get with
`elicitation_not_found`, even when the feature is declared; see
[visibility](../../crates/ck-cingulate/src/lib.rs#L372-L445) and
[operator get](../../crates/ck-cingulate/src/lib.rs#L1642-L1684).

The desktop list sample instead uses a plain local route under the
`dev-local-operator` build, as in [desktop-dev-cards-v1](../desktop-dev-cards-v1/).
Its declared `clientFeatures` are exactly
`["option-group", "secret-use-approval", "manifest-binding-approval"]`. This
development-only admission lets it see `flowInstall.secrets`. The paired phone
and development desktop pages serve `expiresAtMs: 1777593660000` on that row,
as does the phone get. The filing supplies no card expiry, so the
[store uses the earliest statement deadline](../../crates/cingulate-store/src/lib.rs#L484-L509),
not a default lifetime derived from the filing clock.

Successful envelopes are the handler's exact bytes, with no trailing newline.
Inputs retain their original whitespace. Do not edit served bytes by hand.
From the workspace root, regenerate and check the full corpus with:

```sh
cargo run --locked -p cingulate-vectors --features ck-cingulate/dev-local-operator --example generate_basal_flow_install_cards_vectors -- test-vectors/basal-flow-install-cards-served-v1.1
cargo run --locked -p cingulate-vectors --features ck-cingulate/dev-local-operator --example generate_basal_flow_install_cards_vectors -- --check test-vectors/basal-flow-install-cards-served-v1.1
cargo test --locked -p cingulate-vectors --features ck-cingulate/dev-local-operator --test basal_flow_install_cards
```

To regenerate from the original filing samples rather than the archived
`*-request.json` files, pass the filing-samples directory before the served
directory. Its seven inputs are named `<card>.json`.

Without the development feature, the test regenerates the release-route files
and still checks the manifest against every file, including the development
desktop page. After any intentional regeneration, review the byte changes and
update the pinned manifest digest.
