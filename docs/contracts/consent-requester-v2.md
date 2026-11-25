# Consent requester contract, v2

The orchestrator's current module id is `prefrontal`. Cingulate also accepts
`prefrontal-core` in daemon-stamped principals and scope owners, and in stored
answer provenance, so records from before the rename remain usable. These exact
reserved aliases identify the same module; they do not authorize other owners.
Outgoing scope descriptions, session notices and card-settled outcomes pointers
use only `prefrontal`.

Version 2 preserves every v1 operation and body for existing kinds. It adds AFT
permission asks and the optional generic-consent correlation field described below. Pin the [served corpus](../../test-vectors/tool-permission-v1.1/README.md)
and its `SHA256SUMS`; the phone projection is specified in
[consent-phone-v2.md](consent-phone-v2.md). These asks deliberately have no expiry,
retention tombstone, execution report, acknowledgment or settlement-feed entry:
AFT uses durable request/get/await/withdraw and core receives notices.

Package and flow-install `consent.request`/`consent.get` also support the optional
[`explanation` and `changes` author texts](consent-card-explanations-v1.md) when
`consent-card-explanations/v1` is declared. The same objects appear unchanged in
app pending lists; they never appear in notices.

## Federated consent carriers

Since ck-cingulate 0.16.0, Cingulate declares `scope-federated/v1`. An admitted
provider carrying a daemon-stamped `federated` scope may raise `kind: "consent"`
without a local agent id. The subject comes only from the stamp's
`attributes.federated`: `peer_name`, `peer_class`, `peer_fingerprint`, `pairing_id`
and `remote_claim`. No roster fetch, local agent resolution, body-supplied name
or harness string supplies that identity. A present `parent_state` other than
`linked` refuses `elicitation_federated_unlinked`. Other request kinds remain
local-only. Federated routes never inherit local agent, local operator or module
authority. Callosum operator phones and simulator inventory readers use stamped
peer class; unstamped Callosum routes keep the existing harness rules and never
select federated grants.

Only for a card raised on a federated-stamped route, ownership is the admitted
requester principal plus (`peer_fingerprint`, `pairing_id`, `peer_class`,
canonical claim identity). Claim identity includes its kind and is:

- no claim: an explicit claimless identity;
- agent or run: `agent_id` (agent and run remain distinct kinds);
- worker: `owner_module_id` and `worker_ref`;
- flow: all four of `flow_id`, `flow_version`, `flow_install_id`, `flow_code_hash`.

The scope ref/epoch, `peer_name`, worker `on_behalf_of`, `agent_name` and
`flow_name` do not enter that identity. A different provider, pairing, class or
claim cannot collect the card.
Pending `el_` records already persist independently of a route; route closure
cancels its wait, not its card. On a new route with the same paired subject the
provider can get, await, collect answers and acknowledge them through the normal
operations. A rename or re-created claim scope does not orphan a card. Local v1
and v2 ownership remains the principal plus scope owner/ref/epoch, unchanged.
Remote scopes never become targets for local-agent session-ended checks or
local-agent notices.

For federated cards only, `prompt` is an **action phrase**, not a whole sentence.
For example, the provider sends `"prompt":"read your mail."`; for roster name
`Juniper` and stamped agent id `Sage`, Cingulate displays
`“Juniper” says agent Sage wants to read your mail.`. Agent and flow ids are
displayed verbatim, not resolved through this computer's local roster. The
stored `subject.federated` contains the stamped facts, with nested protocol keys
unchanged on both phone and requester projections. Its `subject.label` uses the
[federated permissions label](permissions-v1.md#federated-grants), or just the
peer name for a run; no local `subject.agent_id` or `head_agent_id` is emitted.

| Claim | Displayed prompt for action `read your mail.` |
|---|---|
| agent `Sage` | `“Juniper” says agent Sage wants to read your mail.` |
| worker with `on_behalf_of: "Sage"` | `“Juniper” says a worker for agent Sage wants to read your mail.` |
| worker without attribution | `“Juniper” says one of its workers wants to read your mail.` |
| flow `Morning-digest`, version 3 | `“Juniper” says flow “Morning-digest” (version 3) wants to read your mail.` |
| run for `Sage` | `“Juniper” says a run for agent Sage wants to read your mail. (this time only)` |
| no claim | `A program on “Juniper” (no agent named) wants to read your mail.` |

Every card adds the fact `“Juniper” is paired with this computer.`. “Cloud” and
“hosted” are never added unless present in the person's roster name.

Since 0.16.1, validated optional names from the stamped claim are shown in quotes
with the id beside them in parentheses. Without a name the sentences above are
unchanged. Names are stored as display data in `subject.federated.remote_claim`,
never used as identity. See the exact named sentences and validation rule in
[consent-card-text-v1](consent-card-text-v1.md#federated-caller-names).

A run card is **one immutable answer per elicitation, for the call that raised
it**, not a standing grant. The provider binds the elicitation id (and its
optional `requester_ref` / private `binding`) to that call and MUST NOT reuse its
answer for another call. Answer recovery remains idempotent until normal ack or
retention: a lost reply may be read again. `requester_ref` is still correlation,
not a dedup key or credential. Cingulate sets `single_use: true`, removes
`duration`, requires exactly one `effect: "grant"` option, and labels that option
`Allow this time`, with no provider-authored detail. More than one or no grant
choice is refused. The provider's own `effect: "decline"` options and the card's
default are passed through as sent, so a run card's default is still a decline;
silence, expiry and errors never grant. Providers MUST honor single use and
create no standing grant for runs. Cingulate itself creates no standing grants
for any card. Selectors and base-scope liveness remain the provider's obligations
under [permissions/v1](permissions-v1.md#federated-grants).

## Generic consent and requester correlation

To preserve v1 behavior, the generic-consent rule admits `direct` and
`reserved:cerebellum`, `reserved:plexus`, `reserved:broca` and
`reserved:entorhinal` for
`kind: "consent"` only. These requesters use the generic consent body; none can
request `flow_install`, `flow_decision`, `package`, `tool_permission` or
`macos_permission`. Separately, Cerebellum may file `secret_use` and Plexus may
file `manifest_binding` under the dedicated vault-card rules below. A disallowed
kind refuses `elicitation_requester_refused`.
Requester and subject identity come only from the daemon's route stamp, never
from body fields. A nonblank carried agent id is required for reserved requesters
except for the worker admission rule below and `manifest_binding` as described
in its dedicated section.

For `kind: "consent"` from any requester already admitted for that kind, a stamp
without an agent id may be a `worker` owned by `reserved:prefrontal`.
Cingulate resolves its same-owner stamped `parent` through the daemon's
`scope.describe`, using the permission-ask liveness rules in
[Admission and stamped identity](#admission-and-stamped-identity). The parent (a
worker's parent is always its head) must be live at the stamped epoch, with a
nonblank agent id. A missing parent,
a definitively non-live parent or a parent without an agent id refuses
`access_no_head`; unknown or failed liveness refuses `access_unavailable`.
All other missing-agent stamps retain their existing admission behavior.

The subject is the parent's agent, with its name resolved exactly as on the
head's own card. The stamped worker scope, not the head scope, is recorded as
`asker`: a detached card's later `consent_answered` notice targets the worker's
session. Body fields cannot supply either identity. Requester reads and the core
outcome feed include `subject.via_worker: true` for these cards; phone reads use
`subject.viaWorker: true`. The flag is omitted, not false, on every other card.
Generic worker cards retain the ordinary generic-consent state machine and
fail-closed expiry when a lifetime is supplied. They ask the user directly; they do not first require an
answer from the head, unlike worker permission asks.

All existing v1 kinds, including generic worker consent, may omit
`expires_in_ms` only when Cingulate advertises `consent-no-expiry/v1` in its
`catalog.list` capabilities. Omission (or null) means no deadline: the card stays
pending until answered or withdrawn, with no expiry fields on requester or
phone reads. A present integer retains v1's 1–86,400,000 ms bounds and fail-closed
expiry. Older versions refuse an omitted lifetime, so the capability check is
required. Await timeouts bound only the call; on a no-deadline card neither await
nor `block_until_ms` gains an expiry cap. Detached generic cards can be answered
arbitrarily later and still produce the usual late-answer notice. Permission
asks continue to forbid expiry fields entirely. The optional execution-report
`reason` for existing kinds follows [v1](consent-requester-v1.md#consentreport_execution).
Generic-consent requesters may report after ack while retained. Cingulate's
`approved_not_executed` / `session_ended` backstop fills in only when no report
exists and the stamped asker scope has definitely ended; requester reports always
replace earlier reports, including the backstop, as specified in v1.

`reserved:cerebellum`, `reserved:plexus`, `reserved:broca`,
`reserved:entorhinal` and `reserved:aft` may call `consent.request`,
`consent.await`, `consent.get`, `consent.answers`, `consent.ack`,
`consent.withdraw`, `consent.report_execution`, `consent.detach` and
`consent.release_inputs`. Get, await, release_inputs,
report_execution, withdraw and detach require the raising owner. `consent.answers`
returns only that owner's answers, and `consent.ack` acknowledges only that
owner's answers. None of these requesters can call phone-only `consent.answer` or
`consent.list_pending`, nor read the core-only `consent.outcomes` feed.
Generic `consent` params add two optional top-level fields:

| Field | Rule |
|---|---|
| `requester_ref` | Opaque string of 1–128 printable ASCII characters (bytes `0x20`–`0x7e`), returned unchanged. If present, null, non-string, empty, overlong, control and non-ASCII values refuse `elicitation_invalid_request` with a fixed `requester_ref` field message. Other kinds cannot carry it. |
| `single_use` | Optional boolean accepted only for `kind: "consent"`; null and every non-boolean value refuse `elicitation_invalid_request` with `single_use` in the message. Absence means false. Providers must check `catalog.list` for `consent-single-use/v1` before sending it. |

The reference correlates an originating call with a later answer; it supplies
neither identity nor display text. It is stored durably and returned as
`requester_ref` in requester `consent.get`, `consent.await`, `consent.answers`
and the core `consent.outcomes` feed, as well as requester mutation readbacks.
There it is an opaque correlation key exactly as stored on the card. When absent,
the field is omitted. Phone `consent.list_pending`, `consent.get` and answer
readbacks never include it, in either snake_case or camelCase. Acknowledgment
still controls requester reads only; phone get returns settled generic cards until
their payloads are pruned 24 hours after settlement, as specified by the phone
contracts.

It has no uniqueness or deduplication meaning: two cards may share the same
reference. Only `dedup_key` selects a pending card to refresh. Changing or removing
`requester_ref` on an otherwise matching dedup refresh retains the card id and
updates the stored reference; it does not withdraw and replace the card.

`single_use` is stored with the card and returned on requester reads when supplied.
The core outcome feed includes it only when true. It is never added to a phone
projection, so existing phone card bytes are unchanged.

An outcome record includes `late: true` only when `consent.detach` committed before
the card settled. Settlement captures the stored detach state in the same store
transaction that records the outcome; a later clock reading does not affect it. A
dedup refresh clears detachment, so an answer to the re-raised card is not late.
Core may interpret an absent `late` as “settled while the raiser waited” only when
`catalog.list` advertises `consent-outcomes-late/v1`. Without that capability,
absence means the status was not reported.

The [generic-consent v2 served vectors](../../test-vectors/consent-requester-v2/README.md)
cover requests with and without a reference, worker admission and liveness
refusals, requester and phone records, answers/outcomes, and refusal of a
non-consent request. Their `SHA256SUMS` is
generated from actual handler replies. Existing v1 contracts and corpora remain
byte-for-byte unchanged.

Ordinary card outcomes optionally include `answered_surface: "operator_app"`
when answered through a daemon-recognized desktop operator app. The member is
omitted for phone answers, development-local answers, and historical records.
`answered_device` retains its phone-fingerprint meaning. Phone-style projections
spell the new member `answeredSurface`; neither field takes identity from the
request body. The [outcomes samples](../../test-vectors/consent-outcomes-refs-v1/README.md)
include a desktop answer.

## Admission and stamped identity

Only `reserved:aft` may request `tool_permission` or `macos_permission`.
Other requesters refuse `elicitation_requester_refused`, naming the requester.
AFT's ordinary `consent` filings require a daemon-stamped scope and use its stamped agent
identity, including head resolution for workers; retired agents refuse `consent_retired`.
The `tool_permission`, `macos_permission` and `secret_use` admission rules are unchanged.
For permission asks, an absent scope or a scope owned by anything
other than `reserved:prefrontal` refuses `access_not_alfonso`.
The daemon-stamped scope ref, epoch, kind, agent and route harness are recorded;
no body field supplies identity. Workers do not need an agent id.

A `worker` with a stamped `parent` starts in `pending:head`. A missing or
non-live parent refuses `access_no_head`. A `head` or `ephemeral` starts in
`pending:user`. Worker parent liveness is read from the daemon's `scope.describe`,
not route disappearance or a core lookup. The daemon's `not_live` while its
configured owner has not re-synced is unknown, not proof of death. Unknown or
failed liveness at creation refuses `access_unavailable`; for an already-open
ask it leaves the ask pending. The existing maintenance sweep checks pending
head legs and settles definitive head loss as `access_head_gone`. It also
checks the stamped asking session itself, including worker scopes, for every
pending answer leg. Definitive asker loss settles as `access_asker_gone`,
retracts the user card and queues the usual `settled` notice. Asker probes are
paged and time-bounded; unknown liveness, errors and timeouts keep asks pending
with bounded exponential backoff. Neither sweep compares an ask's age or a
reply deadline. No answer leg expires.

## Request bodies

For permission asks, `params` is a closed object with `kind` and the following fields, not a nested
body. Unknown keys (including `parent`, `scope`, requester identity and expiry)
refuse `elicitation_invalid_request`. The entire framed request retains v1's
1,310,720-byte limit; no field is truncated.

| `tool_permission` field | Rule |
|---|---|
| `call_key` | Required nonblank string; persistent idempotency key. |
| `tool` | Required nonblank string, at most 64 Unicode characters. |
| `operation` | Required `read`, `write` or `exec`. |
| `paths` | Required array of at most 16 absolute `/`-rooted paths; no NUL. Empty is allowed for pathless commands. |
| `command` | Required for `exec`, forbidden otherwise; nonblank, at most 600 characters. |
| `reason` | Required nonblank single line, at most 160 characters; no CR, LF or NUL. |
| `args_hash`, `pattern` | Required nonblank strings, opaque to Cingulate. AFT owns hash and pattern semantics. |
| `schema_pin` | Optional nonblank string; opaque. |
| `head_policy` | Required on workers only: `allow` or `not_held`. AFT attests whether the head holds a covering permission. |

The fixed options are `allow_once`, `allow_scope` (effect `grant`) and `deny`
(effect `decline`, default). Heads also have `escalate`. A head with `not_held`
is offered only `deny` and `escalate`. Unavailable answers are omitted.

`macos_permission` has required string `folder` (absolute, no NUL), `program`
(nonblank), and `steps` (nonblank instructions for System Settings). It refuses
`access_macos_only` outside macOS. It is informational: empty options, no default,
no core park notice, no awaiting call (`consent.await` refuses `access_informational`).
One open card per `(folder, program)` across AFT sessions; a repeat returns the
open id. AFT withdraws it when the folder becomes readable. After withdrawal a
new request can create a new card.

Success remains exactly `{elicitation_id: string}`, with an `acc_` id. For tool
asks, `(raising principal, call_key)` returns the original ask, even if settled
or request content changed, across AFT and Cingulate restarts. The original
fields and identity never change. A replay must still be an admitted scoped AFT
request; a settled replay does not require its original head to remain live.

## Records, await and withdrawal

Requester get uses `elicitation_id` (permission asks also accept `elicitationId`).
The record is flattened: request fields, `elicitation_id`, `created_at_ms`,
`state`, `requester_label`, stamped `asker: {scope_ref, scope_epoch, agent_id?}`,
worker-only `head` of the same shape, fixed `options`, and `default: "deny"`
on tool asks. It omits all expiry fields. Pending records may have
`block_until_ms` for active waits. Escalated records carry `escalation_note`.
Settled records add `outcome`, `answered_by: "head" | "user"`, `answered_at_ms`
and optional `note`. Refusals have no `answered_by` and have their `access_*`
`outcome` and `code`. Explicit `deny` has outcome `deny`, code `access_declined`.

States are `pending:head`, `pending:user`, `settled`,
`refused:access_head_gone`, `refused:access_asker_gone`, or
`refused:access_withdrawn`. An answer never changes
the recorded asker scope. A user's `allow_scope` on an escalated ask applies to
the worker, not its head. Cingulate stores no grants and performs no operation.

Get/await require the raising **principal**, not a mutable transport session.
Another principal refuses `elicitation_not_owner`; an unknown id refuses
`elicitation_not_found`. Records and audit survive indefinitely for recovery.
`consent.await` accepts the id plus v1 `timeout_ms` (0–120,000 after capping),
returns the current requester record when settled or at the caller's wait bound,
is read-only and can be repeated. A returned pending state grants nothing.
Cancelling/dropping a wait removes its transient registration and changes no row.

`consent.withdraw` accepts the id from the raising principal, or, for `tool_permission`
asks only, from `reserved:prefrontal` when the recorded asker scope is owned by
`reserved:prefrontal`. Cingulate checks only that ownership; it cannot see worker
runs. Prefrontal Core withdraws only once a worker run is terminal (delivered, failed
or cancelled), never while the worker is parked waiting on the ask. It moves any
pending state to `refused:access_withdrawn`; a terminal record is returned
unchanged. Withdrawal, answer and head loss are serialized store transactions.

## Core relays and first-answer-wins

Core answers from `reserved:prefrontal` using
`{elicitation_id, answer, note?, answered_by: {agent_id, scope_ref, scope_epoch}}`.
`answered_by` is closed and must exactly match the worker's stamped parent ref
and epoch and its daemon-described agent. Normally only `pending:head` tool asks
accept a relay. `allow_once` and `allow_scope` require `head_policy: "allow"`; `deny` and
`escalate` do not. A refused relay records an attempt but leaves the ask pending.
An optional note is one line, at most 160 characters. `escalate` changes the leg
to `pending:user`, recording the note and newly offered user surface.

While an escalated `tool_permission` ask remains `pending:user`, the head recorded
in its accepted escalation attempt may relay `deny`, with the existing optional
one-line, 160-character note. All three `answered_by` identity fields must match
that escalator exactly; a different head gets `access_wrong_head`, and a record
without a recorded escalator gets `access_wrong_leg`. Head grants and repeated
escalation still get `access_wrong_leg`. This deny settles exactly like a user
deny (`settled`, outcome `deny`, code `access_declined`, usual `settled` notice),
but keeps `answered_by: "head"` and the ordered head escalation/denial attempts in
the audit. It wakes awaiters and removes the phone card. The first user answer or
head deny to settle wins in the same store transaction; the loser gets `conflict`.
Generic consent and macOS cards do not gain a head-answer path.

The user answers through a daemon-recognized `operator_app` route or the
Callosum + `fed-phone:` phone bind rule, only in `pending:user`. The default-off
`dev-local-operator` build feature also permits development-local callers.
No body field can enable these rules. The immediate store
transaction reads the leg, validates authority/options, records the attempt and
compares/sets the new state. The first settlement wins. A later answer, including
the same answer, returns `{ok:false, code:"conflict", elicitation:Record}` with
the committed outcome. Accepted answers return `{ok:true, alreadyAnswered:false,
elicitation:Record}`. Wrong legs use `access_wrong_leg`, wrong parents use
`access_wrong_head`, and an unheld head grant uses `access_head_not_held` in that
same `ok:false` form. Invalid options use `elicitation_invalid_choice`.

## Notices and audit

### Person-facing elicitation cards (`el_`)

Phone-answerable cards with a daemon-stamped asker scope queue an `escalated`
notice in their filing transaction, regardless of the filing module. Cards
requiring `at_console` do not push: a phone has no attested console-machine identity.
The existing subject string supplies the agent name (or resolved fallback), plus
`in project <name>` when the daemon stamp identifies a project. It is truncated
to at most 200 Unicode characters, including a trailing `…` when shortened.

The exact stored JSON has this stable key order (optional keys are omitted):

```json
{"elicitation_id":"el_1","purpose":"escalated","asker":{"scope_epoch":3,"scope_ref":"worker"},"head":{"scope_epoch":7,"scope_ref":"head"},"fields":{"card_kind":"secret_use","subject":"ALF in project Example","expires_at_ms":1700000600000,"phone_only":true,"via_worker":true}}
{"elicitation_id":"el_1","purpose":"settled","asker":{"scope_epoch":3,"scope_ref":"worker"},"head":{"scope_epoch":7,"scope_ref":"head"},"fields":{"outcome":"answered"}}
{"elicitation_id":"el_1","purpose":"withdrawn","asker":{"scope_epoch":3,"scope_ref":"worker"},"head":{"scope_epoch":7,"scope_ref":"head"}}
```

A person card filed without a daemon-stamped session scope (an install card core
files itself, or a local app's card) queues the same notices with `asker` and
`head` omitted, so the person is still told; core pushes it with no agent label:

```json
{"elicitation_id":"el_1","purpose":"escalated","fields":{"card_kind":"consent","subject":"an unverified local process","expires_at_ms":1700000600000,"phone_only":false}}
{"elicitation_id":"el_1","purpose":"settled","fields":{"outcome":"answered"}}
```

Core must accept a missing `asker` on these `el_` notices before a Cingulate
that sends them is deployed.

- `asker`, when present, and optional `head` come only from daemon-attested
  owner scopes. A
  worker's `head` requires a same-owner parent with daemon-confirmed `Linked`
  state; otherwise attribution stays with its asker. Older cards without a saved
  parent omit `head`; no body-supplied session is used.
- `escalated.fields` contains `card_kind`, `subject`, optional `expires_at_ms`,
  `phone_only`, and optional `via_worker`, in that order. Readers must ignore
  members they do not know. `via_worker` is present only when true, indicating
  that a worker asked for the card. `card_kind` is `consent` for consent, package
  and flow-decision cards, `secret_use` for secret use, `manifest_binding` for
  repository bindings, `broker_action` for broker actions, `sandbox_escape` for
  one command outside a sandbox, `config_plan` for configuration plans, and
  `flow_install` for installs (including those carrying secrets). Cards with no
  expiry omit `expires_at_ms`.
- `settled.fields` contains only `outcome`: `answered` or `expired`. Withdrawal
  has no `fields` key. No notice contains requester titles, prompts, scope text,
  statements, vault bytes or choice details.
- Settlement supersedes any pending `escalated` row in the same transaction,
  including expiry sweeps. Startup backfills still-pending cards with
  `INSERT OR IGNORE` on `(elicitation_id, purpose)`; a delivered or superseded
  notice is never replaced. This uses the existing shared `access_notices`
  outbox and delivery loop, without a schema migration.
  An already-backfilled startup reads first and opens no backfill write transaction.

### Permission asks (`acc_`)

A transaction enqueues each notice uniquely on `(elicitation_id, purpose)`.
Cingulate calls core's `session.notice` as `reserved:cingulate`, with the exact
[design wire](../designs/tool-permission-asks.md#the-sessionnotice-wire):
`elicitation_id`, `purpose`, `asker: {scope_ref,scope_epoch}`, worker-only `head`
of the same shape, and typed `fields`. Purposes are `head_ask`, `user_ask`,
`escalated`, `settled`, `withdrawn`. There is no phone push in the head leg.
Settlements withdraw previously offered user surfaces and wake awaiters after
commit. Superseded undelivered opening notices cannot later re-park a settled ask.

Notice delivery is off the state-machine request path. Missing handlers, core
unavailability and transport errors retry from the durable outbox, with capped
exponential backoff (250 ms to 60 s), using the existing maintenance cadence.
This scheduling is not an ask deadline or timed settlement. `unknown_session`
or `session_not_live` on `head_ask` settles `access_head_gone`; on `settled` it
records `unclaimed`. On other purposes it records refusal only. None retry.
`unauthorized_caller` and `invalid_params` are logged defects and never retry.

The durable audit retains original request, requester principal, stamped asker
and head identity, scope kind and harness, creation time, every offered surface
and its answers/time, every answer attempt including losers and malformed
attempts (surface, principal, time, answer, note, relayed identity, result),
escalation note, final state and settlement time. Outbox rows retain delivery
status, attempt count and last refusal. A late answer never overwrites an earlier
outcome or its note. Standing grants remain entirely AFT's responsibility.

A desktop attempt adds optional `via: "operator_app"` provenance to that audit,
including malformed or losing attempts. Its authorization leg remains
`surface: "user"`, and an accepted desktop answer still settles with
`answered_by: "user"`. Phone, head and development-local attempts omit `via`;
older persisted attempts also omit it. The
[permission samples](../../test-vectors/tool-permission-v1.1/README.md) include a
desktop attempt read from the durable audit.

## Late answers on generic consent cards

This detached-call definition does not authorize answers after a deadline.
`secret_use` is refused after its earliest statement expiry, for both allow and
decline; it does not support `consent.detach`.

A requester that raises a generic `consent` card while holding an agent's tool
call usually waits on `consent.await` and returns the answer in that call. When
it stops holding the call and returns "still waiting" to the agent instead, any
later answer has to reach the agent through core. An answer that arrives while
the requester still holds the call must not reach the agent a second time.
The requester marks the moment it stops holding the call with `consent.detach`,
and only settlements after that mark produce a notice.

### `consent.detach {id}`

Admitted only from the principal that raised the card, and only for kind
`consent`. Other kinds refuse `elicitation_requester_refused`; an unknown id or a
card raised by another principal answers with the existing not-found and owner
refusals.

- **Pending card:** detach commits, and the reply is the card record with
  `detached: true`.
- **Already settled card:** detach changes nothing and returns the settled
  record. The requester then returns that answer in the held call, and no notice
  is ever sent for the card.
- **Repeated detach:** always returns the card's current record: still pending
  with `detached: true`, or settled if it settled in between. It never undoes
  the first detach.

Detach and settlement are ordered in one store transaction, so exactly one of
"settled while held" and "settled after detach" applies to every card. A
requester calls detach exactly when it stops holding the call, and must handle
the settled-record reply.

Only the `consent.detach` reply carries `detached`. Every other requester read,
the core outcomes feed and phone reads never do. A dedup refresh that keeps the
card in place clears the detach mark, because the requester is holding a new
call.

### The `consent_answered` notice

When a detached card settles, Cingulate sends core `session.notice` with
purpose `consent_answered`. It is addressed to the agent session whose scope
the subconscious daemon stamped on the route the requester used for
`consent.request`. The body follows the `session.notice` wire described in
"Notices and audit" above (`elicitation_id`, `purpose`, `asker`, `fields`).
The `head` member, which only worker asks carry, is absent:

```json
{
  "elicitation_id": "el_0000000000000042",
  "purpose": "consent_answered",
  "asker": {"scope_ref": "sc_head_alf", "scope_epoch": 3},
  "fields": {
    "requester": "cerebellum",
    "outcome": "answered",
    "choice_id": "allow_30m",
    "choice_label": "Allow for 30 minutes",
    "settled_at_ms": 1792000000000
  }
}
```

| `fields` key | Type and limit |
|---|---|
| `requester` | Module id of the raising principal, without the `reserved:` prefix: 1–64 ASCII characters. |
| `outcome` | `answered` \| `expired` \| `withdrawn`. |
| `choice_id` | The chosen option's `id`, 1–64 characters. Present only when `outcome` is `answered`. |
| `choice_label` | The chosen option's `label`, 1–200 characters, as the operator saw it. Present only when `outcome` is `answered`. Core escapes it before showing it to the agent. |
| `settled_at_ms` | Unsigned integer, Unix milliseconds. |

The notice never carries `requester_ref`, the prompt, facts or the request
body. A card that was never detached never produces this notice. Core
deduplicates on `(elicitation_id, purpose)` and wakes the agent with the
notice; that handling is core's change, made alongside this one. Delivery,
retry and refusal handling are those of the other notices above:
`unknown_session` or `session_not_live` records the settlement as `unclaimed`
and never blocks it.

The target bytes are pinned in
[`test-vectors/consent-late-answer-v1/`](../../test-vectors/consent-late-answer-v1/README.md).
Cingulate's implementation must produce them byte for byte, and core decodes
them.

## Manifest-binding requester (0.18.0)

**Readers first.** The phone and desktop must render `manifest_binding` before
Plexus files one. Phones advertise the separate `manifest-binding-approval`
feature; `secret-use-approval` alone does not opt into this kind.

Only the full daemon-attested principal `reserved:plexus` may file this kind.
Other principals (including bare `plexus`, enrolled identities and Direct) get
the ordinary `elicitation_requester_refused`, before statement decoding.
The filing is a closed object, with exactly one statement:

```json
{"kind":"manifest_binding","manifest_binding":{"statement_b64url":"<vault propose reply, unchanged>"}}
```

No authored title, prompt, facts, display name, requested expiry, or other member
is accepted. In particular, both `expires_at_ms` and `expires_in_ms` are refused
as unknown filing fields. Cingulate derives the card expiry from the statement's
`expires_at_ms`; a filing at or after that time refuses `elicitation_expired`.
There is no expiry override or refresh.

The unpadded base64url statement must decode to RFC 8785 JCS canonical UTF-8 bytes
with exactly `{v, proposal_id, agent_id, app_slug, repo, installation_permissions,
manifest_version, manifest_sha256, prior_version, prior_sha256, expires_at_ms}`.
`v` is `ck-manifest-binding-statement/v1`, `proposal_id` is a ULID, permissions
are a map with values only `read`, `write` or `admin`, the new version is exactly
`prior_version + 1`, and both SHA-256 fields are 64 lowercase hex digits. Integers
are nonnegative and at most 2^53 − 1. Defects refuse
`elicitation_invalid_request` with a reason naming `manifest_binding.<field>`;
invalid encoding or noncanonical bytes name `manifest_binding.statement_b64url`.
Cingulate stores those exact statement bytes (as unchanged base64url) and their
lowercase-hex SHA-256, plus read-only `agent_id`, `app_slug`, `repo` and the full
`installation_permissions` map. It stores no target-agent display name; apps
resolve that name from core's agent record. Generic card text is fixed locally.
The vault's proposal grammar also applies: `agent_id` is 1–128 ASCII letters,
digits, `_` or `-`; `app_slug` is 1–100 lowercase ASCII letters, digits or `-`;
`repo` has exactly two slash-separated parts of 1–100 ASCII letters, digits,
`.`, `_` or `-`, with neither part `.` or `..`.

Plexus may file without a carried agent: the subject is Plexus. When either the
carried agent or scope agent is daemon-stamped, every such agent must equal the
statement's `agent_id`, or the filing refuses `elicitation_invalid_request` with
an `agent_id` reason. Subject identity is never taken from the statement.

Identical statement bytes dedup by `statement_sha256`, for the same full owner
(principal plus session), just as `secret_use` re-requests do. While unexpired,
a repeat returns the existing card id without refreshing it, including after
an answer. A new statement is a new card. A repeat from another session gets
`elicitation_request_conflict`: a card for this statement is already filed under
another session. Do not refile it. Learn an approval from the vault's
`manifest.watch`; otherwise the pending card lapses at the statement's expiry.
Outcome reads do not acquire cross-session authority.

Plexus reads its card through the usual requester `consent.get` with
`{elicitation_id}` or `consent.await` with `{elicitation_id, timeout_ms}`. The
snake_case record has `manifest_binding` facts and, after an accepted answer,
`manifest_binding_answer`:

- allow: `{decision:"allow", statement_sha256, envelope_sha256}`, with the
  route-derived `answered_device` alongside it;
- decline: `{decision:"decline", statement_sha256}`, with `answered_device`
  and no envelope hash;
- expired: `state:"expired"`, no selected choice and no
  `manifest_binding_answer` because no phone answered. Expiry is a denial, never
  an allow.

Only the attested Callosum phone route may answer; the development desktop
operator switch is not authority for this kind. See the
[phone binding contract](consent-phone-v2.md#manifest-binding-approvals-0180).
Cingulate never calls the vault: the phone signs and submits an allow envelope
directly to `manifest.approve`; a decline is recorded only in Cingulate.
Escalate, settle and withdraw push notices contain only Cingulate-vouched
metadata, never the statement, repository or permissions. Their field shapes
are the same as secret-use notices.

[Handler-generated samples](../../test-vectors/manifest-binding-v1/README.md)
include first and repeated filing, phone get and answers, and both requester
get and await records for allow, decline and expired outcomes. The copied vault
statement and envelope are pinned to claustrum tag `manifest-binding-vectors-v1`
at `c2a9ce2eaf99da646160eeb0c73d6b11d9341cbf`.

## Broker-action requester (0.27.0)

See the [broker-action contract](broker-action-v1.md#filing) for scoped filing,
deduplication, the requester record and the separate `expired` outcome.

## Sandbox-escape requester (0.28.0)

Cingulate advertises `sandbox-escape-approval/v1`. Only `reserved:aft` may file
`kind: "sandbox_escape"`, on a route with a daemon-stamped agent scope. A missing
scope or agent, or another filer, refuses `elicitation_requester_refused`. The
agent comes from the route stamp. A worker's card names its stamped head agent,
marks `subject.via_worker: true` and goes straight to the person; the head cannot
answer it. Only an [approving device](consent-phone-v2.md#sandbox-escape-approvals-0280)
may answer. A paired phone needs a verified official app connection to approve
`run_once`; a refused attempt leaves the card pending. The local desktop
operator path is unchanged. An agent or an unsigned surface cannot approve.

The `consent.request` params are a closed object containing only `kind` and
`sandbox_escape`. The nested object is also closed and contains all of the
fields below. Unknown keys (including subject identity, card wording or options)
and invalid fields refuse `elicitation_invalid_request`.

| `sandbox_escape` field | Rule |
|---|---|
| `call_key` | Nonblank string. A valid repeat from the same stamped owner returns the original `elicitation_id` unchanged, even after settlement or restart. It does not refresh the deadline or replace the recorded fields. |
| `command` | Nonblank string, at most 2,000 Unicode scalar values. Line feeds and tabs are allowed. Other Unicode control characters and bidi formatting characters (U+061C, U+200E–U+200F, U+202A–U+202E, U+2066–U+2069) are refused. |
| `cwd` | Absolute `/`-rooted folder, no NUL. |
| `shell` | Absolute `/`-rooted path to the shell program, no NUL. |
| `escapes_level` | `credentials`, `standard` or `strict`. |
| `background` | Boolean. |
| `reason` | One line of text, at most 160 Unicode scalar values. |
| `expires_in_ms` | Integer from 1 to 600,000 inclusive. |

The requester record preserves the exact fields under `sandbox_escape` and
reports the deadline in `expires_at_ms`. The answers are `run_once` (effect
`grant`) and `decline` (effect `decline`); “Don't run” (`decline`) is the default.
No answer by the deadline (at most ten minutes), decline, or withdrawal all
mean no run. The card records those outcomes separately; expiry and withdrawal
have no `answered_choice_id`.

`run_once` covers one run of exactly the recorded command, folder and shell.
Any change to those fields requires a new card with a fresh `call_key`. AFT runs
the command from its own record of the request, never from text read back from
the card. AFT does not file this card for commands carrying secret placeholders
or running on a build server; those use separate approval paths. After approval,
AFT records whether the command ran with `consent.report_execution`.

Examples and refusals are in
[`sandbox-escape-v1`](../../test-vectors/sandbox-escape-v1/README.md).

## Vault secret-use requester (0.12.0)

Cingulate advertises `secret-use-approval/v1`. Only `reserved:aft` and
`reserved:cerebellum` routes with a daemon-attested scope stamp may file
`kind: "secret_use"`; all other requesters or unstamped routes refuse
`elicitation_requester_refused`. No requester or subject identity is supplied in
the body. The complete closed filing body is:

```json
{"kind":"secret_use","secret_use":{"items":[{"request_id":"01HF7YAT000000000000000001","statement_b64url":"<vault statement>"}]}}
```

There are 1–16 items. No title, prompt, labels, options, expiry override or other
filer fields are accepted. Cingulate supplies fixed generic card chrome and
allow/decline options, default `decline`, `on_expiry: "deny"`. The phone renders
access details from statements, with the subject resolved from the route stamp.
Notification pointers contain no secret name, host or agent id.

Each statement is strict unpadded base64url (nonzero trailing bits refused),
decoding to RFC 8785 canonical JSON, byte-identical to its re-encoding. Its
closed key set is `{v, request_id, member_id, scope, agent_id, coverage, secret,
use, durations, request_expires_at_ms}` with `v: "ck-secret-use-statement/v1"`.
Nested closed shapes are `scope: {owner, kind, ref, epoch}`,
`secret: {name, kind, incarnation}`, and `use: {host, path_prefix, location}` for
API keys or `{origin}` for logins, as defined by Claustrum's Agent secrets
contract. Integers are nonnegative and at most 2^53 − 1; floats are refused.
Each canonical 26-character uppercase ULID `request_id` must equal its item's.
`agent_id` equals the stamp's `attributes.agent_id`, including null. Every item
shares one agent and one member, and request ids never repeat.

The card expires at the earliest statement `request_expires_at_ms`. Already
expired statements and answers at or after expiry refuse `elicitation_expired`.
Identical ordered items on the same owner return the existing card without
refreshing its deadline, including after an answer while requests remain live.
Any different filing that reuses a request id refuses
`elicitation_request_conflict`. Request-id reservations survive card retention;
an expired filing is refused even if identical to an older card.

After the phone submits `secret.approve` to the vault using this elicitation id
as `card_ref`, it answers Cingulate with the exact `envelope_b64url`. The vault
verifies the signature, not Cingulate. Cingulate verifies the envelope binding
specified in the [phone contract](consent-phone-v2.md#vault-secret-use-approvals-0120).
A decline carries no envelope. Owned get/await/answers expose `secret_use_answer`:

```json
{"decision":"allow","duration":"hour","items":[{"request_id":"01HF7YAT000000000000000001","decision":"allow"}],"envelope_sha256":"7bcd68a3ec5698ad9c9d11e0efa1d67ef7e7aa6ee18c56d963d5f6b18014f180"}
```

Items are recorded in card order; omitted envelope items are `decline`. A whole
decline records every item as declined and omits duration/hash. Repeating an
answer requires the same choice and binding, not merely another allow envelope;
a different binding returns the existing conflict record. The SHA-256 is over
the decoded envelope bytes and matches the vault's grant and audit
`payload_hash`. See the [pinned vault and served phone vectors](../../test-vectors/secret-use-approval-v1/README.md).


## Flow-install secret approvals (0.13.0)

Cingulate advertises `flow-install-secrets/v1`. Core sends the new fields only
when that capability is present. Only `reserved:prefrontal` may file
`flow_install`, as before. The existing `flow_install.flow` remains the flow's
name; the new identity key is **`flow_identity`**, not `flow`.

Inside the otherwise opaque `flow_install` object, supply both new keys or
neither:

```json
{"flow":"daily-fetch","flow_identity":{"flow_id":"daily-fetch","version":7,"code_hash":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","install_id":"install-A"},"secrets":[{"request_id":"01HF7YAT000000000000000001","statement_b64url":"<unchanged vault flow statement>"}]}
```

`flow_identity` is closed: `flow_id` and `install_id` are 1–256 printable ASCII
bytes, `version` is an integer 1 through 2^53 − 1, and `code_hash` is exactly 64
lowercase hex characters. `secrets` has 1–16 closed `{request_id,
statement_b64url}` objects with unique request ids. Every other install-body key
remains opaque. Cards without these two keys keep their previous behavior.

Basal obtains each statement via the vault's `secret.request_flow_approval`;
core forwards its bytes unchanged. Filing reuses the `secret_use` strict
unpadded base64url and canonical JSON checks (including trailing bits, duplicate
keys, closed shapes and safe integer bounds). The decoded closed statement is
`{v, request_id, member_id, flow, secret, use, durations, request_expires_at_ms}`
with `v: "ck-secret-flow-statement/v1"`. `flow` equals `flow_identity` exactly;
`request_id` equals the entry's ULID; `durations` is exactly `["version"]`.
`secret` has the same closed shape as an agent statement. A login uses
`use: {origin}`; an API key uses `{host, path_prefix, location, methods}`, with
nonempty, unique, ascending uppercase methods of 1–16 bytes each.

Invalid filings store nothing. Named refusal codes are
`elicitation_flow_secrets_pair_required` (one key missing),
`elicitation_invalid_flow_identity` (identity shape/bounds),
`elicitation_invalid_flow_secrets` (entry/statement shape, count or encoding),
`elicitation_duplicate_request_id`, `elicitation_flow_identity_mismatch`, and
`elicitation_invalid_flow_durations`.

Core normally omits `expires_in_ms` under `consent-no-expiry/v1`. A secret-bearing
install still has a deadline: the earliest statement's `request_expires_at_ms`,
or the filed expiry if earlier. Vault flow requests may last up to 24 hours.
An already expired statement refuses `elicitation_expired`; any answer at or
after the deadline does too. Expiry settles the card as `expired` (denied, no
grant), regardless of the supplied `on_expiry`, which is normalized to `deny`.
The decline default is still required.

Only supported phone routes may see or answer these cards, including declines;
see the [phone contract](consent-phone-v2.md#flow-install-secret-approvals-0130).
A successful install answer records `secret_use_answer` with duration `version`,
all items allowed and the exact envelope SHA-256. A decline records all items
declined without duration/hash. The route's device fingerprint is recorded in
`answered_device`. Repeat answers require the same choice and exact envelope
bytes, as for `secret_use`. No storage migration is needed: the existing JSON
record, answer binding and retained secret-card visibility flag hold these
values. See [generated vectors](../../test-vectors/flow-install-secrets-v1/README.md).

## Forwarded secret statements v2 (0.13.1)

When an agent or flow on another paired computer asks for a secret, the home
computer's vault forwards the request and writes a v2 statement. The card is
filed on the asking computer, by its own AFT, Cerebellum or core, to its own
Cingulate, exactly as for v1. A v2 statement is the v1 statement plus one key,
`asking_member_id`, and `v` ends in `/v2`: `ck-secret-use-statement/v2` on a
`secret_use` card, `ck-secret-flow-statement/v2` in `flow_install.secrets`.
`member_id` still names the home vault serving the grant. Canonical bytes,
envelope digests, expiry, idempotency and every other rule above are unchanged.
Legacy requests made on the home computer stay v1, byte for byte.

- A v1 statement must not carry `asking_member_id`, not even as `null`.
- A v2 statement must carry it as `fed-computer:` plus exactly 32 lowercase hex
  characters, different from `member_id` (the home vault never forwards to
  itself).
- A version string from the other family is refused.
- Every statement on one card has the same `asking_member_id`, or none has it,
  so v1 and v2 statements never share a card.

Refusals reuse the existing codes: `elicitation_invalid_request` for
`secret_use`, `elicitation_invalid_flow_secrets` for flow statements. Cingulate
does not compare `asking_member_id` with this computer's identity; the home
vault sets it from the attested forwarding route and grants only a statement
whose digest matches its stored request. Vault vectors and generated cards:
[secret_use](../../test-vectors/secret-use-approval-v2/README.md) and
[flow install](../../test-vectors/flow-install-secrets-v2/README.md).

## Extended secret statements v3 (ck-cingulate 0.16.2)

Extended scopes use `ck-secret-use-statement/v3` for `secret_use` and
`ck-secret-flow-statement/v3` in `flow_install.secrets`, for both local and
forwarded requests. All other fields retain their v1 meanings, except that
`asking_member_id` is present exactly for forwarded requests, with the same
grammar and serving-vault exclusion as v2. Per-card asker consistency is
unchanged; a `secret_use` card also retains its one-agent check. Every statement
on a secret-use or flow-install card must name the same serving vault, because
one home vault holds all agent secrets and flow grants for the account.

The closed `use` shapes are version-specific:

| Version | API key | Login |
| --- | --- | --- |
| v1/v2 | `{host, path_prefix, location}` | `{origin}` |
| v3 | `{rules: [{host, path_prefix}, ...], location}` | `{origins: [...]}` |

Flow API keys additionally require the existing sorted, nonempty `methods` list
beside the legacy fields or `rules`. Every v3 rule must contain both string keys;
`path_prefix: ""` means no prefix. For v3, `secret.kind: "agent_api_key"` requires
the API-key shape and `"site_login"` the login shape; any other kind refuses.
Unknown keys, mixed shapes and shapes under the wrong version refuse with the
existing invalid-statement codes. Cingulate checks shape/version agreement and
card binding, not host grammar, scope counts, public suffixes or scope matching:
the vault remains the authority on the signed scope.

For flow statements, an extended v3 request cannot use either v3 statement tag as
`flow_id`, local or forwarded. A forwarded request (v2 or v3) also cannot use
either v2 statement tag as `flow_id`. A local v3 request may use a v2 tag; local
legacy v1 flow ids remain unchanged.

Envelope bytes, signatures, approvals and the recorded `envelope_sha256` are
unchanged. Cingulate does not render statement `use` content into card text or
labels; it relays all statement bytes unchanged to the phone, which renders the
complete scope. The [pinned vault samples](../../test-vectors/secret-statements-v3/README.md)
cover canonical multi-rule and wildcard scopes and multiple origins.

## Config-plan requester (0.21.0)

**Readers first.** Callosum checks `catalog.list` for `consent-config-plan/v1`
and CKIOS reads the [typed-section samples](../../test-vectors/config-plan-v3/README.md),
including every new member. Callosum files the new typed program-change flags,
rollback program detail and `route_rebind` transition only after the phone's reader
is live; until then Callosum keeps filing them as other sections (Decline-only). Only the full daemon-attested principal
`reserved:callosum` may file, get, await, withdraw or report on `config_plan`.
Every other principal is refused `elicitation_requester_refused`. Callosum may
file no other kind. No carried agent or session scope is required. Ownership is
Callosum's principal, not its transport session or scope. Federated requester
scopes cannot file these cards. The daemon is the subject: **This computer's
settings**. Even if the filing route has a scope, the card has no agent, asker or
`head` attribution field (the asking worker's parent session), and produces no
agent/session notices. Scope-less person-facing escalated,
settled and withdrawn notices still drive phone pushes/retractions, as on other
cards. Notification metadata never includes change values or signed bytes.

`consent.request` params are closed, including the typed plan and every nested
section object and row. Unknown keys refuse `elicitation_invalid_request`. Example:

```json
{"kind":"config_plan","expires_in_ms":300000,"binding":{"correlation":"fake-correlation"},"config_plan":{"plan_digest":"fake-plan-digest","base_revision":"42","apply":{"operation":"change","rollback_to":null,"modules":[],"review":null},"changes":[{"module_id":"example-module","project_id":"example-project","path":"settings.enabled","old":false,"new":true,"old_source":"default","new_source":"project","apply":"live"}],"other_sections":{"policy_changes":[{"example":true}]}}}
```

| Field | Rule |
|---|---|
| `expires_in_ms` | Required integer, 1–300,000. No omitted/null or longer lifetime. |
| `binding` | Required opaque JSON object, at most 4,096 serialized UTF-8 bytes. Returned only to the requester, never the phone. |
| `dedup_key` | Optional, with the existing pending-card dedup/refresh behavior. |
| `config_plan.plan_digest` | String, 1–128 UTF-8 bytes. |
| `config_plan.base_revision` | Nonempty decimal string (ASCII digits only). Not a JSON number. |
| `config_plan.changes` | Required array of 0–64 closed field-change rows. |
| row `module_id`, optional `project_id` | Strings, 1–128 UTF-8 bytes each. A present project id cannot be null. |
| row `path` | String, 1–1,024 UTF-8 bytes. |
| row `old`, `new` | Required recorded JSON values, including null; at most 4,096 serialized UTF-8 bytes each. |
| row `old_source`, `new_source`, `apply` | Strings, 1–64 UTF-8 bytes each. `apply` is open; known values are `live`, `module_restart`, `daemon_restart`, `next_supervision`, `route_rebind`. Unknown values are preserved. |
| `config_plan.apply` | Required closed object with four required members: `operation`, `rollback_to`, `modules`, `review`, and optional `rollback_programs`. Present null is invalid. |
| `apply.operation`, `rollback_to` | Operation is exactly `change` or `rollback`. `rollback_to` is a nonempty ASCII decimal string exactly for rollback; otherwise it is null. Both members are required. |
| `apply.modules` | Required array of 0–64 closed rows, unique by `module_id` (1–128 UTF-8 bytes). Every row requires all seven members below. |
| module `transition` | Exactly `live`, `module_restart`, `route_rebind`, `next_start` or `daemon_restart`; unknown transitions (including `next_supervision`) refuse. Unlike a field row's open `apply` string, this is a closed enum. |
| module `whole_generation`, `waits_for_next_start`, `waits_for_daemon_restart` | Required booleans. |
| module `deferred_live_fields`, `explanation` | Required array of 0–64 paths (1–1,024 UTF-8 bytes each), and required plain-text explanation (1–512 UTF-8 bytes). |
| `apply.review` | Required null or closed object `{"supervisor_reconcile":true}`; false is invalid. |
| `apply.rollback_programs` | Optional array of 0–64 closed rows `{module_id, input, approved_then, on_disk_now}`, allowed only for `operation: "rollback"`, even when empty. Module id is 1–128 UTF-8 bytes; input is 1–1,024 UTF-8 bytes. Both values are required recorded JSON, including null/redaction, with the same 4,096-byte cap as change-row `old`/`new`. No `reason` member; Callosum drops the daemon's reason before filing. |
| `config_plan.affected_projects` | Optional array of 0–64 unique project ids, each 1–128 UTF-8 bytes. |
| `config_plan.policy_changes` | Optional array of 0–64 closed rows: `kind`, conditional `module_id`/`layer`, required `old`/`new`, optional `reason`. |
| policy `kind`, `module_id` | Kind is exactly `declaration`, `subc_jsonc`, `reviewed_program` or `layer_file`. `module_id` (1–128 UTF-8 bytes) is required except for `subc_jsonc`, where it must be absent. |
| policy `old`, `new`, `reason` | For non-`layer_file` kinds, values have the same JSON/redaction form and 4,096-byte cap as field changes. Optional reason is at most 512 UTF-8 bytes. |
| `layer_file` policy `layer`, `old`, `new` | `layer` is required, 1–64 UTF-8 bytes, and allowed only on this kind. Each value is a closed `{"absent":true}` or `{"sha256":"<64 lowercase hex>","bytes":<non-negative integer>}` object. `reason` must be absent. |
| `config_plan.launch_manifests` | Optional array of 0–64 closed entries `{module_id, origin?, programs, program_changed_since_target?, program_changed_since_active?}`, unique by `module_id` (1–128 UTF-8 bytes); optional origin is 1–64 UTF-8 bytes when present, never null. |
| manifest `programs` | Required array of 0–16 closed rows `{input, old, new}`; input is 1–1,024 UTF-8 bytes. Values have the same JSON/redaction form and 4,096-byte cap as field changes. Every entry needs at least one program or one program-change flag. A flag-only entry carries `programs: []` and may omit origin. |
| manifest `program_changed_since_target`, `program_changed_since_active` | Optional flags, present only as literal `true`; false, null and every other value refuse. Either or both flags mean the module restarts on the plan's program. |
| `config_plan.other_sections` | Optional object of section name to raw JSON, at most 16,384 serialized UTF-8 bytes in total. Present null is invalid. The phone displays unknown sections raw. |

`apply` explains when settings take effect and which restarts or reconciliation
are needed. Its explanation renders as literal plain text, not HTML or Markdown;
as with other card explanations, controls U+0000–U+001F and U+007F–U+009F
other than U+000A newline are refused,
and whitespace is preserved. `affected_projects` identifies the projects touched by the plan.
`policy_changes` exposes changes to authority separately from ordinary settings;
`subc_jsonc` records changes to the daemon's subc.jsonc policy configuration.
its `layer_file` kind records settings-file edits with no effective setting change,
including comments, formatting, same-value replacements, creation and removal.
`launch_manifests` shows the programs that will be launched, so approval covers
executable changes as well as settings. The phone words `program_changed_since_target`
as "Its program changed since that version. It restarts on the program now on disk."
and `program_changed_since_active` as "It is running an older program. It restarts
on this plan's program." `apply.rollback_programs` shows which program will run,
comparing what was approved then with what is on disk now. A `route_rebind` module
keeps running; its connections close and reopen under the new settings before
the change counts as active. Phone wording: "This module's open connections close
and reopen under the new settings." The daemon's `next_supervision` is mapped by
Callosum to `next_start`, not added to the closed transition set.
`changes` shows the field-level values
and their sources; `other_sections` retains anything the typed reader cannot yet
render, keeping such a card Decline-only.

At least one row must exist across `changes`, `policy_changes` and
`launch_manifests` (whose entries each need a program or flag); restart scope,
rollback program detail or affected projects alone is not a plan. Missing required members, invalid types/enums, duplicate ids, empty plans
and every exceeded cap refuse `elicitation_invalid_request` at filing, never
truncate. Optional lists and optional row strings cannot be present null.
The phone must render every section before approval; nonempty `other_sections`
remains Decline-only. Historical stored cards without `apply` still read, but
offer only decline, and approval attempts refuse `elicitation_invalid_choice`.

The existing 1,310,720-byte frame bound remains. Cingulate fixes the card presentation and
options: `approve` (effect `choose`) and `decline` (effect `decline`, default),
with `on_expiry: "deny"`. Silence, timeout, expiry and errors never approve.
Success is `{"elicitation_id":"el_1"}`. Use owned `consent.get
{elicitation_id}` or `consent.await {elicitation_id,timeout_ms}` (0–120,000 ms)
to read the snake_case record. Pending records start at `answer_seq: 0`.
Each new accepted answer increments `answer_seq`; identical retries of the same
settled choice and signed bytes do not increment it. Conflicting retries never
replace an answer. After an approval, reads include these fields unchanged:

```json
{"answer_seq":1,"answered_choice_id":"approve","statement":"_wABeCBub3QtSlNPTgo","signature_b64url":"AQIDBA"}
```

This is a record excerpt; full get/await replies are in the
[handler-generated samples](../../test-vectors/config-plan-v3/README.md).
Every typed section is stored and returned unchanged in requester get/await and
phone get/list/watch, including reopened rounds. Nested member names stay snake_case.
`statement` is unpadded base64url of at most 4,096 decoded bytes;
`signature_b64url` is unpadded base64url of at most 1,024 decoded bytes. Present
empty, malformed, padded, noncanonical trailing-bit or overlong encodings are
refused `elicitation_invalid_request`. Approve requires both; decline may carry
either or neither. Cingulate stores and returns the exact strings byte for byte;
it checks only encoding and size, never parses the statement, canonicalizes its
content, checks its decision or verifies its signature. **Only Callosum verifies.**

Phone `consent.answer` accepts optional `expectedRound`, a non-negative JSON
integer in the same camelCase style as `elicitationId`. Omission means round 0.
Inside the recording transaction, Cingulate first compares it to the card's
current `round` (also 0 when omitted on the card). A mismatch returns the typed
answer result `{"ok":false,"code":"round_changed","detail":{"currentRound":1}}`
without recording an answer. The phone must reread the card and obtain a new
decision for its current round. Other card kinds ignore this member, preserving
the existing handling of unrelated answer params.

After the round check, an exact retry of the current round's recorded choice
and signed strings remains `alreadyAnswered: true`. Otherwise, the SHA-256 of
the decoded statement bytes is checked against every recorded answer on this
card, across all rounds and choices. A reused digest returns
`{"ok":false,"code":"statement_reused"}` without recording anything, even if
the signature or choice differs. A new answer in a reopened round needs both
the matching `expectedRound` and fresh statement bytes. These are typed answer
result refusals, not code/message transport errors. Cingulate already stores
the digest in `config_plan_audit`; no statement content is interpreted and no
new schema migration is required.

### `consent.report_unverified`

Callosum reports an answer that failed verification:

```json
{"elicitation_id":"el_1","answer_seq":1,"code":"signature"}
```

Params are closed; `code` is exactly `time`, `plan`, `decision`, `signature` or
`key`. The card must be answered and still within its original expiry. A stale
or nonexistent `answer_seq` refuses `elicitation_invalid_request`, naming
`answer_seq`; an expired card refuses `elicitation_expired`. Wrong kind or
principal refuses `elicitation_requester_refused`. A valid report atomically
returns the card to pending for its remaining lifetime, removes the old current
answer/feed row, advances the person-notice round and preserves prior answer metadata
in the audit. No old choice or signed bytes remain in the pending projection.
The response is the current requester record; it and the phone projection add:

```json
{"round":1,"unverified":{"code":"signature","reported_at_ms":10000}}
```

The phone shows “The approval couldn't be verified. Nothing was changed.” A new
answer clears `unverified`, receives `answer_seq: 2`, and is verified again by
Callosum. A repeated report on an already pending card is refused. Awaiters and
pending revisions are notified on reopening just as for a new pending card.
Phone list/watch/get records and requester get/await records carry the same
`round` as the current round's person notices: a JSON integer reopen count,
omitted on the initial round and never emitted as 0. The value remains on that
round's answered or otherwise settled record. A higher round supersedes a final
copy of the same elicitation id and shows it as pending again; `answer_seq` is
an answer-attempt counter, not a substitute for this record identity rule.
Each reopened `config_plan` sends a fresh scope-less `escalated` person notice
and phone pointer with `eventAtMs` equal to `reported_at_ms`, not the original
creation time. Person lifecycle notices have an optional nonnegative integer
`round`: the number of times the card has reopened. Round zero is omitted, so
existing notices and the initial round remain byte-identical. The outbox and
Core's intake, phone-push receipts and settled/withdrawn checks key each notice
by `(elicitation_id, purpose, round)`, interpreting an omitted round as zero.
Earlier rounds remain as outbox history. Each round queues its own `settled`
notice when the phone answers; that notice is delivered before the next round's
`escalated`, including across delivery retry backoff. The exact four-notice
sequence is in `person-notices-two-rounds.json` in the served samples. Core must
ship round-aware intake and phone-push handling before reopening is deployed.
`consent.withdraw` and the expiry sweep retract pending cards normally. No
execution report, detach, ack or answer-feed operation is admitted to Callosum.
Payloads are pruned 24 hours after settlement, as for manifest bindings. The audit
retains choice, sequence, answer time, statement SHA-256 and decoded length, and
unverified code/time, never statement or signature bytes.

## Sealed text input (0.23.0)

An ordinary `kind: "consent"` card may carry an optional `inputs` array:

```json
{"inputs":[{"kind":"sealed_text","id":"password","label":"Sudo password","max_bytes":64,"hpke":{"suite":"X25519-HKDF-SHA256/HKDF-SHA256/ChaCha20-Poly1305","public_key":"<unpadded-base64url-32-bytes>"}}]}
```

There is at most one input. Its `id` follows the existing option-id rule
(nonblank string); `label` is a nonblank string. `max_bytes` is a JSON integer
from 1 through 4,096, limiting the plaintext's **UTF-8 bytes**, not characters.
The suite string must match exactly; the public key must be canonical unpadded
base64url decoding to exactly 32 bytes. The array and its input/HPKE objects
have closed shapes. Present null, malformed fields, unknown fields, extra
inputs, and inputs on any other card kind refuse `elicitation_invalid_request`
before a card exists. In particular, `secret_use`, `manifest_binding`, `broker_action`, `sandbox_escape`,
`config_plan`, flow-install/package and `tool_permission` cards cannot carry
inputs: tool permissions can be answered by an agent rather than the person,
so they cannot collect a person's typed secret.
AFT is admitted to ordinary consent so it can file the one-time public key, on a
daemon-stamped agent scope. Served samples for this scoped identity are in
[`sealed-text-v1.2`](../../test-vectors/sealed-text-v1.2/README.md); v1 and v1.1 remain unchanged.

For all `ConsentRequest` options, `id` is 1–64 Unicode characters and `label` is
1–200 Unicode characters. Both must be nonblank and contain no control characters
(U+0000–U+001F or U+007F–U+009F); ids must be unique.

Get, list and watch cards carry the input definitions exactly as filed; nested
member names stay snake_case on phone and desktop too. `round` is unchanged
(ordinary cards remain round 0, omitted as today). Notices carry at most
`"input": true`, or omit it; they never contain a public key, `enc`, ciphertext
or answer fingerprint. No notice, outbox, audit row or log includes sealed bytes.

The person answers through the existing phone/desktop operator operation:

```json
{"method":"consent.answer","params":{"elicitationId":"el_1","choiceId":"allow_once","expectedRound":0,"inputs":[{"id":"password","enc":"<unpadded-base64url-32-bytes>","ciphertext":"<unpadded-base64url-sealed-bytes>"}]}}
```

An approving (`effect: "grant"`) choice requires exactly the filed input; a
decline requires none (absent or empty array). Input on a card without a filed
input is invalid. Cingulate checks only shape: matching id, canonical unpadded
base64url `enc` decoding to exactly 32 bytes, and ciphertext decoding to between
17 and `max_bytes + 16` bytes inclusive. It does not decrypt, verify HPKE, or
interpret plaintext. Malformed/missing/extra answer inputs refuse
`elicitation_invalid_answer` and leave the card open. Existing `expectedRound`
handling is unchanged (ordinary consent ignores it). Exact retries of the
recorded choice and the same `enc`/ciphertext bytes return the recorded outcome
with `alreadyAnswered: true`, without a new settlement, write or notice. A
different settled answer conflicts as before.

Only the daemon-stamped filer can read encrypted answer bytes, through
`consent.get` / `consent.await`. The filed definitions remain in `inputs`; the
separate `answer_inputs: [{id, enc, ciphertext}]` holds the sealed answer. Every
other projection, including phone/desktop read-back, feeds and
`permissions.list_all`, can show only `input_supplied: true` in place of the
answer bytes. No requester-body identity can confer access.

The filer calls `consent.release_inputs {elicitation_id}` after consumption.
It deletes sealed bytes from the answer row and returns `{"released":true}`;
repeat calls return the same result. A non-filer is refused by existing
owner/operator admission (`elicitation_not_owner` or
`elicitation_requester_refused`). Filer reads after deletion omit `answer_inputs`
and show `input_released: true` instead; other readers retain only
`input_supplied: true`. Release does not change the recorded decision.

Withdrawal and expiry also delete the encrypted answer, even if already
answered. A durable sweep deletes it at the latest ten minutes after settlement;
startup runs that sweep so downtime cannot extend the window. Reads also sweep
before projection. Only an unsalted SHA-256 fingerprint of the full recorded
answer (choice, id, decoded enc and ciphertext bytes, length-prefixed) remains
in the answer row, preserving exact retries after release. Neither the
fingerprint nor the encrypted payload is copied to the card record or change
log. See [real-handler samples](../../test-vectors/sealed-text-v1/README.md).
