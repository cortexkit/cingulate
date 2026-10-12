# Phone consent contract, v2

Version 2 includes the [v1 interface](consent-phone-v1.md) unchanged for existing
kinds, plus `tool_permission` and `macos_permission`. It is a new version rather
than claiming additive compatibility: these kinds have new states, no lifetime
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
asks appear in `consent.list_pending` and phone `consent.get` **only** while in
`pending:user`. A worker's `pending:head` ask is not phone-visible. Settled and
withdrawn permission ids refuse `elicitation_not_found` on phone get; AFT retains
the durable record. An answer reply still returns the committed terminal record
for first-answer-wins recovery. Existing kinds retain v1 get retention behavior.

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
- `asker: {scope_ref, scope_epoch, agent_id?}`, worker-only `head` of the same
  shape; these are daemon facts, not body identity;
- `options` using v1 `{id,label,effect}`; labels are the answer ids and callers
  must submit ids, not effects;
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
request is capped at 1,310,720 bytes, so a card fits a page. Nonexpiring cards
sort after expiring cards, then by id; the existing continuation object's
`expires_at_ms` is only a sort position for them (`9223372036854775807`), never
an expiry on the card. Copy continuations unchanged; never derive timeouts from
them. Polling revisions account for permission state changes and active waits.

## Answering

Phone params remain `{elicitationId, choiceId, note?}`. `note` on permission asks
is an optional single-line string of at most 160 characters. Unknown unrelated
params do not create authority. Accepted user answers require `pending:user`.

| Attempt | Result |
|---|---|
| Valid user-leg tool answer | `{ok:true, alreadyAnswered:false, elicitation:Card}`. |
| Head leg | `{ok:false, code:"access_wrong_leg"}`; no state change or hidden card body. |
| Already terminal, even the same choice | `{ok:false, code:"conflict", elicitation:Card}` with the first outcome. |
| Invalid answer or any answer on an informational card | `{ok:false, code:"elicitation_invalid_choice", elicitation:Card}`. |
| Unknown id | `{ok:false, code:"not_found"}`. |
| Non-phone caller | Transport refusal `elicitation_operator_required` (except `reserved:prefrontal-core` relays, whose parent identity and leg are checked separately). |

A terminal answer projection adds `outcome`, `answeredBy: "user" | "head"`,
`answeredAtMs`, optional `note`. Explicit denial adds `code: "access_declined"`.
Refused records carry their named `access_*` outcome and code and no answeredBy.
States are `settled` or `refused:access_head_gone` / `refused:access_withdrawn`.
Never infer a grant from elapsed time, missing cards, a failed get, or a transport
error. If an answer reply is lost, refresh the list; AFT and core can recover the
durable final outcome. Phone get deliberately does not reveal a card off its
user leg. V1 answer recovery/retention remains valid for the existing kinds only.
