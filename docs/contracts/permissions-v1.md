# permissions/v1 and the permissions page

Status: contract, not yet implemented. Vectors:
[`test-vectors/permissions-v1/`](../../test-vectors/permissions-v1/README.md).
Consumers: Cerebellum (first provider), cingulate (aggregator), the phone and the
desktop app (the permissions page), Callosum (expose profiles).

The key words MUST, MUST NOT and MAY are normative. Field names are snake_case,
as in the brief that set these shapes; card references use cingulate's
elicitation ids.

## Why

Standing grants stay with the provider that enforces them (design ruling 12).
The operator still needs one page that lists every grant across modules and can
revoke any of them. `permissions/v1` is the provider side of that page; cingulate
fans out to the providers and serves the page.

## Provider capability

A module that holds standing grants declares `permissions/v1` in its manifest's
`capabilities.provides`. Callers find providers by capability, never by module
name. Cingulate itself does not provide `permissions/v1`.

### `permissions.list {agent_id?}`

Returns `{grants: [row, ...]}`, live grants only, filtered to one agent when
`agent_id` is given ([`provider-list`](../../test-vectors/permissions-v1/provider-list.json)).

```json
{"id": "grt_02", "agent_id": "agent-alf",
 "holder": {"kind": "agent", "agent_id": "agent-alf"},
 "module": "cerebellum", "subject_label": "Whole computer",
 "scope": "whole_computer",
 "scope_label": "the whole computer, including your mouse and keyboard",
 "group": "danger", "granted_at_ms": 1000000, "card_ref": "el_2"}
```

| Field | Meaning |
|---|---|
| `id` | Opaque, stable for the grant's life. |
| `agent_id?` | The agent the grant belongs to; for a worker grant, the owning head. Absent for an operator-held grant. |
| `holder` | `{kind: "agent" \| "worker" \| "operator", agent_id?, scope_ref?}`. See worker grants below. |
| `module` | The provider's module id, so the page can address a revoke. |
| `subject_label` | What the grant covers, for display: an app name, a site, "Whole computer". |
| `scope` | A token from the provider's declared vocabulary. Cerebellum declares exactly `app`, `whole_computer`, `site` and `all_sites`. |
| `scope_label` | A human sentence for the grant. Surfaces show it as given and never compose wording from `scope`. |
| `group?` | `"danger"` for grants as broad as the danger option that minted them. Cerebellum sets it on `whole_computer` and `all_sites`, and only there. |
| `expires_at_ms?` | Absent for a grant without expiry ("always"). |
| `granted_at_ms` | When the current form of the grant was minted. |
| `card_ref?` | The `elicitation_id` of the card whose answer minted it. |

Row rules:

- **One row per subject per holder.** Upgrading "30 minutes" to "always" replaces
  the row in place: same `id`, no `expires_at_ms`, new `granted_at_ms` and
  `card_ref` ([`provider-list-after-upgrade`](../../test-vectors/permissions-v1/provider-list-after-upgrade.json)).
  Keeping the id means a revoke sent for the row the operator saw also ends the
  upgraded grant, the narrower outcome.
- **Expiry.** A grant drops out at `expires_at_ms`: a row is listed only while
  `now < expires_at_ms` ([`provider-list-after-expiry`](../../test-vectors/permissions-v1/provider-list-after-expiry.json)).

### `permissions.revoke {id}`

Returns `{outcome: "revoked"}` or `{outcome: "not_found"}`. Unknown and expired
ids both answer `not_found`, never a module fault. A revoke that ends work running
under the grant answers `{outcome: "revoked", stopping: true}`; the work's final
state shows on the next read. Revoking is idempotent: a second revoke answers
`not_found`.

Vectors: [`provider-revoke-revoked`](../../test-vectors/permissions-v1/provider-revoke-revoked.json),
[`provider-revoke-stopping`](../../test-vectors/permissions-v1/provider-revoke-stopping.json),
[`provider-revoke-not-found`](../../test-vectors/permissions-v1/provider-revoke-not-found.json),
[`provider-revoke-expired`](../../test-vectors/permissions-v1/provider-revoke-expired.json).

### Provider admission

Revoking is operator authority. A provider admits `permissions.list` and
`permissions.revoke` only from `reserved:cingulate` and from the local operator's
`direct` route, and refuses every other caller with
`permissions_operator_required`
([`provider-revoke-refused-agent`](../../test-vectors/permissions-v1/provider-revoke-refused-agent.json)).
An agent's own narrower self-revoke operations, where a provider has them, are
separate operations and are not part of this capability.

### Worker grants

A grant answered for a worker is held by the worker's daemon-verified scope:
`holder: {kind: "worker", agent_id: <owning head>, scope_ref}`. `scope_ref` is
the scope reference from the daemon's route stamp; it is stable across the
worker's revival and new for each retry. The row is listed under the owning head's
`agent_id`. A worker grant is never keyed by any field the worker can set itself.

## Cingulate aggregator

### `permissions.list_all {agent_id?, since_version?}`

Cingulate calls `permissions.list {agent_id?}` on every module that provides
`permissions/v1` and replies:

```json
{"version": "<hex>", "modules": [
  {"module": "cerebellum", "status": "ok", "grants": [ ... ]},
  {"module": "plexus", "status": "unreachable"}]}
```

- `modules` is sorted by module id, one entry per provider.
- `status` is `ok` (with `grants`), `unreachable` (the call failed, timed out or
  faulted) or `unsupported` (the module declares the capability but does not
  serve `permissions.list`). A module that could not be read is named by its
  status and carries no `grants`; it is never returned as an empty list.
- `version` is the lowercase hex SHA-256 of the RFC 8785 bytes of `modules`.
  Clients treat it as opaque. When `since_version` equals the current version the
  reply is `{version, unchanged: true}` without `modules`.

Vectors: [`list-all-ok`](../../test-vectors/permissions-v1/list-all-ok.json),
[`list-all-unreachable`](../../test-vectors/permissions-v1/list-all-unreachable.json),
[`list-all-unsupported`](../../test-vectors/permissions-v1/list-all-unsupported.json),
[`list-all-unchanged`](../../test-vectors/permissions-v1/list-all-unchanged.json).

### `permissions.revoke {module, id}`

Cingulate forwards `permissions.revoke {id}` to `module` and replies with
`outcome`:

| outcome | When |
|---|---|
| `revoked` | The provider answered `revoked`. |
| `stopping` | The provider answered `revoked` with `stopping: true`. The revoke is ending running work; the final state shows on the next read. |
| `already_gone` | The provider answered `not_found`. |
| `module_unreachable` | The provider could not be reached or faulted. |
| `refused` with `code` | `not_a_provider` when `module` does not provide `permissions/v1`; otherwise the provider's error code. |

Vectors: [`revoke-revoked`](../../test-vectors/permissions-v1/revoke-revoked.json),
[`revoke-stopping`](../../test-vectors/permissions-v1/revoke-stopping.json),
[`revoke-already-gone`](../../test-vectors/permissions-v1/revoke-already-gone.json),
[`revoke-module-unreachable`](../../test-vectors/permissions-v1/revoke-module-unreachable.json),
[`revoke-refused-not-provider`](../../test-vectors/permissions-v1/revoke-refused-not-provider.json).

### Authority

Revoke never prompts: it only narrows authority, so no card is raised. The
request carries no authority field and there is nothing to mint. Authority is the
route stamp, the same authority `consent.answer` uses today:

- `permissions.revoke` is admitted from `reserved:callosum` with the
  operator-surface stamp (the paired phone, through Callosum's `phone` expose
  profile) and from the local operator's `direct` route
  ([`revoke-direct`](../../test-vectors/permissions-v1/revoke-direct.json)).
- `permissions.list_all` is admitted from `reserved:callosum` with the
  operator-surface stamp (the `phone` or `sim` expose profile) and from `direct`.
  The list is the operator's whole grant inventory, so a paired Mac relaying
  through Callosum, which carries no operator-surface stamp, can't read it
  ([`list-all-refused-no-operator-surface`](../../test-vectors/permissions-v1/list-all-refused-no-operator-surface.json)).
- Everything else is refused with `permissions_operator_required`, including
  `reserved:callosum` without the operator-surface stamp and every agent route
  ([`revoke-refused-not-operator`](../../test-vectors/permissions-v1/revoke-refused-not-operator.json),
  [`revoke-refused-no-operator-surface`](../../test-vectors/permissions-v1/revoke-refused-no-operator-surface.json),
  [`list-all-refused`](../../test-vectors/permissions-v1/list-all-refused.json)).

The `direct` admission is not proof that a person acted; see "Known gaps" in
[`docs/STATUS.md`](../STATUS.md).

### Ambiguous drops

`permissions.revoke` is a `mutate` operation, so a phone call crossing Callosum
is ledgered by the federation's effects-v1 ledger: exactly once, with
`effect_status` to learn the outcome of a call whose reply was lost (Callosum
`docs/fed-wire.md`, the `effects-v1` feature and `effect_status` /
`effect_status_result`, §8.8). After an ambiguous drop the caller reads
`effect_status`, or reloads `permissions.list_all`. It never resends blind. The
phone never removes a row optimistically: a row leaves the page only when a read
no longer lists it.

## Requirements on Callosum's expose profiles

For CALLO's Callosum expose profiles:

- `permissions.list_all` reaches cingulate from the `phone` and `sim` profiles.
- `permissions.revoke` reaches cingulate from the `phone` profile only. It is a
  mutating operation, and simulators carry query operations only (design.md,
  "Simulators can't answer").
