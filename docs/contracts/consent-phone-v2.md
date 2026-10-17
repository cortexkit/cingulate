# Phone consent contract, v2

Version 2 includes the [v1 interface](consent-phone-v1.md) unchanged for existing
kinds, plus `tool_permission` and `macos_permission`. It is a new version rather
than claiming additive compatibility: these kinds have no lifetime
fields, and late same-choice answers conflict. Clients pinned to v1 must not
render unknown kinds as ordinary expiring approvals. The shared
[served vectors](../../test-vectors/tool-permission-v1/README.md) pin both phone
and requester bytes. The [requester v2 contract](consent-requester-v2.md) specifies
admission, identity, state machine and core notices.

Generic consent also admits worker sessions without their own agent id, as
specified below. Cards raised by other sessions and existing served vectors
remain unchanged.

## Identity and visibility

The existing operator rule is unchanged: `reserved:callosum` on a daemon-stamped
harness starting `fed-phone:`. No body field establishes authority. Permission
asks appear in `consent.list_pending` **only** while in `pending:user`, projected
as `state: "pending"`. Phone `consent.get` continues to serve a permission ask
after settlement if its durable surface history includes the user leg. A worker's
ask that never reached the user leg refuses `elicitation_not_found`, both before
and after settlement. Removing a card from pending lists does not hide its final
phone get projection. Existing generic consent kinds follow the v1 rule: phone get
continues to return a settled card after requester acknowledgment until the
24-hour retention prune removes its payload.

## Generic consent raised by a worker

Generic `kind: "consent"` cards raised by a daemon-stamped worker without its own
agent id have the live parent head's agent as `subject`. Phone projections add
`subject.viaWorker: true`; it is omitted, not false, on all other cards. The
phone shows the lead agent's name from `subject.label` and indicates that a
worker asked. Cingulate only serves card data; the phone client renders the
consent UI. The stamped worker remains the requesting session; answering a
detached card notifies that worker, not its
head. These cards appear in the ordinary pending list, have a fail-closed expiry,
and accept an operator's answer without first asking the head to answer. Settled
cards retain the ordinary get and same-choice answer recovery behavior specified
in [v1](consent-phone-v1.md). Request, requester get, phone list/get and
outcome bytes are pinned in the
[generic-consent v2 corpus](../../test-vectors/consent-requester-v2/README.md).

## Permission card shape

Top-level phone keys are camelCase, nested payloads retain their spellings.
There is no `request` wrapper. Common permission fields:

- `elicitationId` (`acc_` opaque id), `kind`, `createdAtMs`, `state`,
  `requesterLabel: "reserved:aft"`;
- optional `requesterName`, the human-readable name of a recognized requester
  principal (`AFT` for permission asks). It is omitted for `direct` or unknown
  principals; the raw `requesterLabel` is unchanged and is never used as a fallback;
- `asker: {scope_ref, scope_epoch, agent_id?}`, worker-only `head` of the same
  shape; these are daemon facts, not body identity;
- `options` using v1 `{id,label,effect}`; labels are human-readable and callers
  must submit ids, not labels or effects;
- `blockUntilMs` only while a requester wait is active; this is a wait bound, not
  expiry and never approval;
- `escalationNote` on an escalated card, if the head supplied one.

Tool cards carry `callKey`, `tool`, `operation`, `paths`, optional `command`,
`reason`, `argsHash`, optional `schemaPin`, `pattern`, worker-only `headPolicy`.
Display these as untrusted data. There is no synthetic title, prompt, target or
expiry. `options` are exactly `allow_once` and `allow_scope` (effect `grant`) and
`deny` (effect `decline`); `default` is `deny`, never auto-submitted. Phone cards
never offer `escalate`. Grants apply to `asker` only, including an escalated worker.

Informational macOS cards carry `folder`, `program`, `steps`, `options: []` and
no default. Render the program/folder and System Settings instructions, not an
approve button. AFT closes the card with withdrawal when access works again.

There are no `expiresAtMs`, `expiresInMs`, `onExpiry` or timeout outcome fields.
Phone lists keep the same 3 MiB page budget, package-body omission and revision
protocol as v1. Permission cards need no large-body omission; their full framed
request is capped at 1,310,720 bytes, so a card fits a page. All pending cards,
including permission asks and generic cards with or without deadlines, sort
oldest first by creation time, then lexicographic id. Continuations are JSON
objects `{created_at_ms, elicitation_id}`. Copy them unchanged, including unknown
keys; never derive timeouts from them. The old expiry-keyed object is accepted
on input for one release as specified in v1. Polling revisions account for
permission state changes and active waits.

## Answering

Phone params remain `{elicitationId, choiceId, note?}`. `note` on permission asks
is an optional single-line string of at most 160 characters. Unknown unrelated
params do not create authority. Accepted user answers require `pending:user`.

While an escalated `tool_permission` ask waits for the user, its recorded escalating
head may deny it through the core relay, with an optional note under the existing
limits, only when agent id, scope ref and epoch match the accepted escalation
attempt. Missing escalator identity refuses conservatively; a head cannot grant
after escalating. This settles as `deny` / `access_declined`, with `answeredBy: "head"`
and the usual `settled` notice and ordered answer-attempt audit, wakes the
worker and removes the card from pending lists. Whichever of the user answer and
head deny settles first wins; the other gets the existing `conflict` reply. Generic
consent and informational macOS cards are unchanged.

| Attempt | Result |
|---|---|
| Valid user-leg tool answer | `{ok:true, alreadyAnswered:false, elicitation:Card}`. |
| Head leg | `{ok:false, code:"access_wrong_leg"}`; no state change or hidden card body. |
| Already terminal, even the same choice | `{ok:false, code:"conflict", elicitation:Card}` with the first outcome. |
| Invalid answer or any answer on an informational card | `{ok:false, code:"elicitation_invalid_choice", elicitation:Card}`. |
| Unknown id | `{ok:false, code:"not_found"}`. |
| Non-phone caller | Transport refusal `elicitation_operator_required` (except `reserved:prefrontal-core` relays, whose parent identity and leg are checked separately). |

A terminal phone projection keeps `outcome`, `answeredBy: "user" | "head"` when
answered, `answeredAtMs`, and optional `note`. Explicit denial adds
`code: "access_declined"`; refused records retain their named `access_*` outcome
and code. For an ask that was presented to the user:

- A user answer projects `state: "answered"`, with top-level `answeredChoiceId`
  equal to the chosen option id (`allow_once`, `allow_scope`, or `deny`) and
  `answeredAtMs`. The three options remain exactly as on the pending card:
  `allow_once` / "Allow once", `allow_scope` / "Allow for this task", and
  `deny` / "Don't allow", including their unchanged effects.
- Any non-user ending, including worker or scope-owner withdrawal, head-scope
  termination, or the escalating head's denial, projects `state: "withdrawn"`
  without `answeredChoiceId`. Tool options remain present; informational macOS
  cards keep their empty options. The projection does not attribute a head's
  denial to the user.

Permission asks have no timer or expiry-like settlement path, so they never
project `expired`. These phone-only mappings do not change stored, requester,
core, or head states or first-answer-wins. If an answer reply is lost, phone
`consent.get` retrieves the same committed final card; it does not re-answer it.
Never infer a grant from elapsed time, missing cards, a failed get, or a transport
error. V1 answer recovery/retention remains valid for the existing kinds.
