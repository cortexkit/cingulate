# permissions/v1 and the permissions page

Status: Cingulate aggregator implemented in 0.5.0; provider operations are specified
here but implemented by the grant-holding modules. Vectors:
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
| `agent_id?` | The local agent the grant belongs to; for a local worker grant, its supervising local agent (the head). Absent for operator, flow, module, enrolled and federated grants. |
| `holder` | An open tagged object: `kind` is `agent`, `worker`, `operator`, `flow`, `module`, `enrolled`, `federated`, or a future kind. See the kind-specific fields below. |
| `module` | The provider's module id, so the page can address a revoke. |
| `subject_label` | What the grant covers, for display: an app name, a site, "Whole computer". |
| `scope` | A token from the provider's declared vocabulary. Cerebellum declares exactly `app`, `whole_computer`, `site` and `all_sites`. |
| `scope_label` | A human sentence for the grant. Surfaces show it as given and never compose wording from `scope`. |
| `group?` | `"danger"` for grants as broad as the danger option that minted them. Cerebellum sets it on `whole_computer` and `all_sites`, and only there. |
| `methods?` | A non-empty list of uppercase HTTP method names, such as `["GET"]`. A provider sends it only for grants it limits by method. Surfaces show it as given. |
| `expires_at_ms?` | Absent for a grant without expiry ("always"). |
| `granted_at_ms` | When the current form of the grant was minted. Always present: a reader drops a row without it, which would hide a grant the person can't then revoke. |
| `granted_at_bound?` | `"at_or_before"` when the provider doesn't know the real mint time, only that it is no later than `granted_at_ms` (for example, rows a store migration rewrote). Surfaces then show the time as an upper bound, such as "granted before 3 Nov". Absent means `granted_at_ms` is exact. A reader that doesn't know a value shows `granted_at_ms` as given and keeps the row. |
| `source?` | Who decided the grant: `person` (a person answered a card or ask), `head_grant` (a head passed on, to its worker, a grant a person gave the head), `head_policy` (a head gave its worker access from its own default policy, with no person deciding), or `unknown` (the provider didn't record it, for example rows saved before it did). Absent means the provider doesn't report sources. The vocabulary is open: a reader treats a value it doesn't know as `unknown`. Surfaces may group or label rows by source, but MUST still list and offer revoke for every row, whatever its source. |
| `card_ref?` | The id of the Cingulate card whose answer minted it: an `elicitation_id` (`el_…`) for a consent card, or an access ask id (`acc_…`) for a folder or permission ask. Absent when no card minted it, for example a grant from a worker's launch plan. Surfaces treat it as opaque. |
| `effect?` | `"deny"` for a [standing denial](#standing-denials). Absent for every grant. Providers emit it only after the readers-first step in that section. |

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

### Flow grants

A grant held by an installed flow version has no agent and no expiry, so it
carries neither the top-level `agent_id`, nor `holder.agent_id`, nor
`expires_at_ms`. Its holder is:

```json
{"kind": "flow", "flow_id": "flow_web_index", "flow_version": 3,
 "flow_install_id": "flowinst_9c41d2"}
```

| Holder field | Meaning |
|---|---|
| `flow_id` | Opaque string naming the flow. |
| `flow_version` | A positive integer of at most 2^53 − 1, the installed version of that flow. |
| `flow_install_id` | Opaque string naming that installation. |

The grant lasts while that installed flow version exists, so the row has no
expiry: retiring the installation, or installing a new version of the same flow,
ends it. A new version is a new holder, and the one-row-per-subject-per-holder
rule applies per version: rows for two versions of one flow are two rows, even
for the same subject. The row's `module` is still the provider that enforces the
grant and answers the revoke, which need not be the module that installed the
flow.

Flow rows appear only in an unfiltered `permissions.list`. They are not listed in
a `permissions.list {agent_id}` reply, even for a flow that an agent installed:
a caller filtering by agent asks for one agent's grants, and a flow row names no
agent.

### Module and enrolled grants

A grant can be held by a program rather than an agent, such as a vault
category grant that lets a module read the user's provider logins. Such a row
names no agent, so it carries neither the top-level `agent_id` nor
`holder.agent_id`. Its holder is one of:

```json
{"kind": "module", "module_id": "broca"}
{"kind": "enrolled", "name": "claustrum-auth-pi"}
```

| Holder field | Meaning |
|---|---|
| `module_id` | With `module`: the bare id of a supervised module whose identity the daemon verifies at launch (the attested principal `reserved:<module_id>`, written without the prefix). |
| `name` | With `enrolled`: the name of a program the person approved by enrolment, such as a login plugin inside OpenCode or Pi (the principal `enrolled:<name>`, written without the prefix). The daemon does not supervise it, so its name is only what the person approved. |
| `label` | Optional, with either kind: a display name the provider writes, such as "Thalamus" or "Claude sign-in for Pi". Plain text, at most 80 characters. A surface shows it when present and otherwise shows the bare `module_id` or `name`; it never keeps its own table of names. |

Like flow rows, module and enrolled rows appear only in an unfiltered
`permissions.list`, never in a `permissions.list {agent_id}` reply. The row's
`module` is the provider that enforces the grant and answers the revoke. A
surface labels a `module` holder as a CortexKit module and an `enrolled` holder
as a program the person approved. A provider lists only grants a person can
safely revoke; keys a provider needs to run at all stay out of the list.

### Federated grants

A federated holder is a separate authority layer, never a local agent or module
grant. The provider authenticates only daemon-stamped `ScopeKind::Federated`
facts, refuses a present `parent_state` other than `linked`, and keys on the
pairing, peer class and selector:

`parent_state` describes the federated claim scope's link to its base scope.
`linked` means the parent is live at the named epoch and the daemon permitted
the link. Pending verification after a restart and ended links grant nothing.

```json
{"kind":"federated","peer_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
 "pairing_id":"pairing-invented-1","peer_class":"computer",
 "selector":"exact_claim","claim":{"kind":"agent","agent_id":"Sage"},
 "label":"Juniper · Sage"}
```

`peer_fingerprint` is 32 lowercase hex characters. `pairing_id` is a non-secret
opaque token, new on every pairing. `peer_class` is the stamped `computer`,
`phone`, `sim` or `unknown`, not a harness-derived class. Every field must equal
the current stamp; reclassification never retains a previous class's authority.
Neither `reserved:callosum` without a federated stamp nor any local scope matches
a federated grant. On an unstamped Callosum route, harness `callosum-self` means
Callosum's own local calls; `fed-<class>:<fingerprint>` means a legacy forwarded
peer call, never Callosum's module authority. Existing phone operator and sim
inventory rules still apply to those legacy routes, not to stamped ones.

| `selector` | Matching rule |
|---|---|
| `exact_claim` | Requires a grantable `claim`, and matches only its canonical identity. |
| `claimless` | Forbids `claim`; matches only a stamp with no remote claim. |
| `all_claims` | Forbids `claim`; matches claimless and grantable claims for this pairing. Recorded only after an explicit choice; providers default to not offering it. |

Missing or unknown selectors match nothing. A `run` claim is never grantable and
is never covered by any selector, including `all_claims`. Remote claims have
these canonical identities (all fields are compared, including the kind):

- `agent`: `agent_id`.
- `worker`: `owner_module_id` and `worker_ref` (64 lowercase hex characters).
  `on_behalf_of` is display only, never authority inherited from that agent.
- `flow`: `flow_id`, `flow_version` (integer 1..2^53−1), `flow_install_id` and
  `flow_code_hash` (64 lowercase hex characters).

The remote ids are opaque printable non-space ASCII tokens, 1–256 bytes; worker
owners use the daemon's module-id grammar: 1–255 UTF-8 bytes, not `.` or `..`,
with no control characters, `/` or `\\`. The peer name, worker attribution,
`agent_name` and `flow_name` are display only. A federated row MUST omit both the
top-level `agent_id` and `holder.agent_id`; remote ids remain only inside
`claim`. These rows appear only
in an **unfiltered** `permissions.list`, never an agent-filtered list. Selector
and canonical claim identity are part of row identity and revoke: two different
selectors are distinct holders and a revoke targets only the opaque row id.

`holder.label` is plain text, at most 80 characters (truncate longer labels to
80 characters). Use the person's roster name first; never add “cloud” or
“hosted” unless those words are in that name. With roster name `Juniper`:

| Claim / selector | Label |
|---|---|
| agent `Sage` | `Juniper · Sage` |
| worker attributed to `Sage` | `Juniper · worker for Sage` |
| worker without attribution | `Juniper · worker` |
| flow `Morning-digest`, version 3 | `Juniper · flow Morning-digest v3` |
| claimless | `Juniper · any program` |
| explicitly chosen `all_claims` | `Juniper · everything on it` |
| run | No row, no standing-grant label. |

Since ck-cingulate 0.16.1, `claim` may also carry optional `agent_name` (agent or
worker) or `flow_name` (flow). A worker's name refers to `on_behalf_of`, not its
worker identity, and is rendered only when that id is present. Providers carry
names only from the daemon-stamped claim. Like module holders, federated holders
use the existing optional `holder.label` for display, not a second label field
or an app-maintained lookup table. The id remains beside the quoted name:

| Named claim | Label |
|---|---|
| agent `Sage`, name `Remote Sage` | `Juniper · “Remote Sage” (Sage)` |
| worker for `Sage`, agent name `Remote Sage` | `Juniper · worker for “Remote Sage” (Sage)` |
| flow `Morning-digest`, version 3, name `Daily digest` | `Juniper · flow “Daily digest” (Morning-digest) v3` |

Absent or invalid names preserve the unnamed labels above byte for byte. Names
follow the [card plain-text rule](consent-card-text-v1.md#federated-caller-names):
1–80 Unicode scalars, no control or bidi formatting characters. Invalid names
are omitted, never grounds for granting or refusing authority. The whole label
still has the existing 80-scalar truncation limit. `holder_label` rechecks names
before rendering. Cingulate relays provider labels unchanged, as it does a
provider-written module label; it does not resolve remote names from a local
roster. Missing labels fall back to the peer and canonical claim ids, not names
supplied separately by a requester.

Names never participate in selectors, canonical row identity, revocation keys
or grant matching. A different name or no name keeps the same grant; the same
name on a different id cannot match it. Display changes may change the listing
snapshot `version` but never its authority. The named reader sample is
[`federated-holders-named`](../../test-vectors/permissions-v1/federated-holders-named.json).

#### Liveness belongs to the grant-holding provider

Before applying a grant, listing it, or committing a consent answer into a grant,
the provider describes `fed-base:<peer_fingerprint>:<pairing_id>` through
`scope.describe`. Declaring `scope-federated/v1` admits that description even
after daemon restart and without having served the scope.

In a describe answer, `owner_synced` means the scope owner has supplied its
current scope inventory in this daemon incarnation. `owner_configured` means
that owner is configured to run on this daemon; false may mean maintenance,
not an ended pairing. An epoch identifies one lifetime of a scope reference.

| Description | Apply | List | Commit answer into grant |
|---|---|---|---|
| live | Admit subject to the provider's ceilings and severity rules | List | Commit |
| ended, or not live with `owner_synced: true` | Refuse | Delete grant | Refuse; no grant |
| not live with `owner_synced: false`, not live with `owner_configured: false`, or describe fails | Refuse | Fail the whole provider call, never a partial list | Refuse; no grant |
| any other answer | Refuse | Fail the whole provider call | Refuse; no grant |

A provider with no federated grants makes no liveness check. Cingulate turns a
failed provider call into `status: "unreachable"`, not an empty or partial list.
Revoke and apply MUST be serialized at the provider so revocation during a check
cannot admit work. A re-pair changes the base reference and never inherits old
grants. Verification loss and its return move the base to fresh epochs, closing
old routes without ending the base scope: grants remain
stored and unused while no call can arrive. When a provider first enables
`scope-federated/v1`, it converts old
`fed-computer:<fp>` grants to `claimless` for that pairing, never `all_claims`.

Handler-generated vectors: `federated-holders`,
`federated-holders-agent-filtered`, `federated-provider-liveness-unavailable`,
and `federated-revoke`. Cingulate only relays provider rows; declaring the scope
capability does not itself create a federated grant or row.

### Standing denials

A standing denial is a row that refuses a federated caller instead of admitting
it. It lets the person answer an ask with "don't allow, and don't ask again" and
see and undo that choice on the Permissions page next to their grants. Calls
that wait for an offline computer need this: they never wait for a card, so
without a lasting answer the same ask would come back on every call.

```json
{"id":"grt_09","effect":"deny",
 "holder":{"kind":"federated","peer_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
   "pairing_id":"pairing-invented-1","peer_class":"computer",
   "selector":"exact_claim","claim":{"kind":"agent","agent_id":"Sage"},
   "label":"Juniper · Sage"},
 "module":"plexus","subject_label":"Mail","scope":"mail_read",
 "scope_label":"never read your mail","granted_at_ms":1000000,"card_ref":"el_9"}
```

Rows:

- **Federated holders only.** A provider sends `effect: "deny"` only on a
  `federated` holder. Denials for other holder kinds are not defined.
- **Same row rules as a grant.** A denial has an opaque `id`, is listed while
  live, may carry `expires_at_ms`, and is revoked with `permissions.revoke {id}`.
  It never carries `group: "danger"`. The [liveness rules](#liveness-belongs-to-the-grant-holding-provider)
  apply unchanged, so a re-pair starts with no denials, just as it starts with
  no grants.
- **One row per subject per holder.** Turning an allow into a deny, or the
  reverse, removes the old row and mints a new `id`. A revoke is addressed to
  the row the person saw, and that row no longer describes the authority.
- **Revoking a denial** removes it. The subject then has no standing answer for
  that holder, so the provider asks again as its own contract says. It never
  turns into a grant.
- **Minted only by an explicit choice.** A provider records a denial only when
  the person picks a standing deny option on a card. A card that expires, times
  out, is withdrawn or is dismissed never mints a denial, even though its
  default answer is decline. The option's label says the answer is lasting, for
  example "Don't allow, and don't ask again".
- **`scope_label`** is the provider's sentence for the denial, such as "never
  read your mail". Surfaces show it as given.
- **Names** follow the federated rules above: display only, never part of the
  row's identity or matching.

Precedence. For one call, the provider takes its live rows for that pairing,
peer class and subject whose selector matches the call (see the selector table
above). Both grants and denials count:

1. A row is **specific** when its selector is `exact_claim` (for a call with a
   claim) or `claimless` (for a call without one). An `all_claims` row is
   **broad**.
2. If any specific row matches, decide from the specific rows only: refuse if
   one is a denial, otherwise admit.
3. Otherwise decide from the broad rows: refuse if one is a denial, otherwise
   admit.
4. If no row matches, there is no standing answer. The provider asks, or
   answers `consent_required`, as its own contract says.

So a denial beats a grant at the same or a broader selector, and an
`exact_claim` grant beats a broader denial. A `run` claim is never covered by
any row, grant or denial, so a run is never refused or admitted by a standing
answer. A call refused by a denial answers `consent_denied` and raises no card.

Readers first. A reader that predates this section would show a denial as a
grant, which tells the person the opposite of the truth. So:

- Phone and desktop Permissions pages MUST show a row with `effect: "deny"` as
  refused ("Not allowed"), never as access granted, and offer the same revoke.
- A row with an `effect` value a reader does not know MUST be kept and MUST NOT
  be shown as granting access.
- Absent `effect` means a grant, as before.
- No provider emits `effect` until the Cingulate aggregator, the phone app and
  the desktop app are confirmed tolerating it in their running builds. Cingulate
  relays `effect` unchanged, and it is part of the listing `version`.

Vectors: [`provider-list-federated-deny`](../../test-vectors/permissions-v1/provider-list-federated-deny.json)
(a provider listing with a grant and a denial) and
[`federated-precedence`](../../test-vectors/permissions-v1/federated-precedence.json)
(a decision table of rows, call and expected answer for providers to test against).

### Holder kinds a reader does not know

`holder.kind` is open: a provider may later hold grants under a kind this
contract does not define. A reader that meets an unknown `holder.kind` MUST keep
the row, MUST NOT drop it and MUST NOT fail the list, and MUST NOT show it as
operator-held. Only `holder.kind: "operator"` is operator-held; a reader shows
an unknown kind as held by something other than the operator, from whatever
`holder` fields it can read, and never guesses at the missing ones.

Readers ship first: phone and desktop Permissions pages MUST tolerate unknown
kinds before any provider emits a `federated` row. Ignore unknown kind-specific
semantics, not the row itself; the keep-row rule above still applies. Cingulate
relays unknown holder kinds without changing or rejecting them.

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
- For a row whose holder kind is `module` and that carries no `holder.label`,
  Cingulate fills `holder.label` from that module's `display_name` in the
  daemon's `catalog.list`, when the module is registered and declares one. A
  label the provider wrote always wins. A surface therefore reads only
  `holder.label` and falls back to the bare `module_id`.
- `version` is the lowercase hex SHA-256 of the RFC 8785 bytes of `modules`,
  after labels are filled, so a module renaming itself changes it. Clients
  treat it as opaque. When `since_version` equals the current version the
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

`module_unreachable` covers both a provider that never received the revoke and
one whose reply was lost, so the grant may or may not be revoked until the list
is reloaded. Every provider Error frame maps to `refused` with its code unchanged;
transport failures, lost replies and unreadable replies map to `module_unreachable`.

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

The daemon's Callosum harness prefixes are `fed-phone:` for the phone and
`fed-sim:` for a simulator. Cingulate derives simulator admission separately
from consent's phone-only operator-surface flag. Neither `fed-computer:`
(spelled `fed-mac:` by Callosum builds before the rename) nor `fed-unknown:`
admits either permissions operation; the prefixes grant nothing
under a principal other than `reserved:callosum`.

### Aggregator refusals

| code | Meaning |
|---|---|
| `permissions_operator_required` | The route lacks the authority required for this operation. |
| `permissions_unavailable` | The daemon's `catalog.list` failed or timed out; the complete provider inventory is unknown. This whole-request refusal is retryable, never an empty inventory. On revoke no provider request was sent, so retrying is safe, unlike `module_unreachable`. |

The original contract-authored vectors remain byte-for-byte unchanged. Additive
`served-*` vectors in the same corpus are captured from Cingulate's real handler
with fake catalog entries and providers. Regenerate or check both sets with
`cargo run -p cingulate-vectors --example generate_vectors --locked` (add
`-- --check` to check without writing).

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
