# Permissions v1

Wire vectors for the `permissions/v1` provider capability and cingulate's
aggregator operations behind the apps' permissions page. The normative contract
is `docs/contracts/permissions-v1.md`. Contract-authored cases pin provider and
aggregator replies. The `served-*` cases and `list-all-holder-labels` are captured
from Cingulate's real handler with fake daemon catalogs and provider replies.

## Encoding

Same as `../danger-option-group-v1/`: each case is `<case>.json` (pretty, sorted
keys, one trailing newline), `<case>.jcs` (RFC 8785, no newline, the pinned form)
and `<case>.sha256` (lowercase hex SHA-256 of the `.jcs` bytes, no newline).
ASCII strings and integers only. `SHA256SUMS` covers every file except itself,
including this README; verify from this directory with
`shasum -a 256 -c SHA256SUMS`. `crates/cingulate-vectors/tests/pins.rs` pins its
SHA-256.

Regenerate or check from the workspace root:

```sh
cargo run -p cingulate-vectors --example generate_vectors --locked
cargo run -p cingulate-vectors --example generate_vectors --locked -- --check
```

`crates/cingulate-vectors/tests/contracts.rs` re-derives every `.jcs`
independently, checks every `.sha256`, recomputes each `version`, and checks the
contract's row invariants.

## Case fields

`tag` is `permissions-v1`; `method`, `route`, `request`, `reply` and `error` are
as in the danger corpus (`route` is test notation for the daemon's stamp, never a
request field). `now_ms` is the provider's or cingulate's clock when the
operation runs; it is not a wire field.

## Scenario

Cerebellum holds five grants minted at 1000000 from answers to cards `el_1` to
`el_5`. Claustrum, an illustrative grant holder here, holds three more: two held
by an installed flow version and one under a holder kind this contract does not
define.

| id | holder | scope | expires |
|---|---|---|---|
| `grt_01` | agent `agent-alf` | `app` (TextEdit) | 30 minutes |
| `grt_02` | agent `agent-alf` | `whole_computer`, group `danger` | never |
| `grt_03` | agent `agent-alf` | `site` (github.com) | never |
| `grt_04` | agent `agent-alf` | `all_sites`, group `danger` | 30 minutes |
| `grt_05` | worker `scope:wkr-7f3a` of `agent-alf` | `app` (Preview) | 30 minutes |
| `grt_06` | flow `flow_web_index` v3, install `flowinst_9c41d2` (module `claustrum`) | `api_key`, `methods` `["GET"]` | while that version exists |
| `grt_07` | flow `flow_web_index` v4, install `flowinst_1b77e0` (module `claustrum`) | `site_login`, no `methods` | while that version exists |
| `grt_08` | unknown kind `schedule` (module `claustrum`) | `api_key` | not stated |

A flow row carries no top-level `agent_id`, no `holder.agent_id` and no
`expires_at_ms`: the grant lasts while that installed flow version exists, and a
new version of the same flow is a new holder. `grt_06` and `grt_07` are two
versions of one flow, so they are two holders. `methods` is present only on a
grant limited by method. `grt_08` is a holder kind from a later version of the
contract: a reader keeps it, never drops it and never shows it as operator-held.
All ids, flow names and labels in these rows are invented.

Provider cases (Cerebellum and Claustrum serving `permissions/v1`, called by
cingulate):

| Case | What it pins |
|---|---|
| `provider-list` | `permissions.list` at 1060000: all eight rows, including both flow holders and the unknown holder kind. |
| `provider-list-after-upgrade` | At 1120000 the TextEdit grant was upgraded to always by card `el_6`: same id `grt_01`, no `expires_at_ms`, new `granted_at_ms` and `card_ref`. |
| `provider-list-after-expiry` | At exactly 2800000 (the 30-minute expiry) the three expiring rows are gone. The flow and unknown-kind rows stay: neither carries an expiry. |
| `provider-revoke-revoked` | `permissions.revoke` of the site grant. |
| `provider-revoke-stopping` | Revoking the whole-computer grant while work runs under it: `revoked` with `stopping: true`. |
| `provider-revoke-not-found` | An id the provider never issued. |
| `provider-revoke-expired` | `grt_01` after its expiry: `not_found`, the same as unknown. |
| `provider-revoke-refused-agent` | A route that is neither `reserved:cingulate` nor `direct`: refused. |

Aggregator cases (cingulate, called by the phone through Callosum unless noted):

| Case | What it pins |
|---|---|
| `list-all-ok` | `permissions.list_all`: one module, `ok`, five rows. |
| `list-all-holder-labels` | Real-handler reply with exactly three rows: a module holder named from the catalog, an enrolled holder with a provider label, and a module holder whose catalog entry declares no name. |
| `list-all-unreachable` | `plexus` could not be read: named by status, no `grants`. |
| `list-all-unsupported` | `aft` declares the capability but does not serve `permissions.list`. |
| `list-all-unchanged` | `since_version` equal to `list-all-ok`'s version: `unchanged`, no `modules`. |
| `list-all-refused` | An agent's carrier route: refused. |
| `list-all-refused-no-operator-surface` | `reserved:callosum` without the operator-surface stamp (a paired Mac): refused. |
| `revoke-revoked` | Provider answered `revoked`. |
| `revoke-stopping` | Provider answered `revoked` with `stopping: true`. |
| `revoke-already-gone` | Provider answered `not_found` (here: `grt_01` after expiry). |
| `revoke-module-unreachable` | The named module could not be reached. |
| `revoke-refused-not-provider` | The named module does not provide `permissions/v1`. |
| `revoke-direct` | The local operator's `direct` route is admitted. |
| `revoke-refused-not-operator` | An agent's carrier route: refused by name. |
| `revoke-refused-no-operator-surface` | `reserved:callosum` without the operator-surface stamp: refused by name. |

Plexus and AFT appear only as illustrative module ids for the unreachable and
unsupported statuses; neither is committed to serve `permissions/v1` here.
Claustrum likewise appears only as the provider holding the flow and
unknown-kind rows; it is not committed to serve `permissions/v1` here. The flow
rows appear only in the unfiltered `provider-list` read, never in one filtered by
`agent_id`.
Revoke carries no authority field and there is no code to mint: authority is the
route stamp, and ambiguous drops are resolved through Callosum's effects-v1
ledger, not by resending (see the contract).
