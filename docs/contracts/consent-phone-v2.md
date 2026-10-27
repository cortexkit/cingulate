# Phone consent contract, v2

Version 2 includes the [v1 interface](consent-phone-v1.md) unchanged for existing
kinds, plus `tool_permission` and `macos_permission`. It is a new version rather
than claiming additive compatibility: these kinds have no lifetime
fields, and late same-choice answers conflict. Clients pinned to v1 must not
render unknown kinds as ordinary expiring approvals. The shared
[served vectors](../../test-vectors/tool-permission-v1/README.md) pin both phone
and requester bytes. The [requester v2 contract](consent-requester-v2.md) specifies
admission, identity, state machine and core notices.

`secret_use` has a hard deadline: both allow and decline are refused at or after
the earliest statement expiry, even if the same choice was already recorded.
Late-answer rules never extend a vault request's lifetime.

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

## consent.watch_pending

`consent.watch_pending` replaces polling `consent.list_pending`. It is a
management `query` that the manifest declares with watch delivery
(`operation_delivery: {"consent.watch_pending": "watch"}`), so Callosum relays
it as a subscription: the request stays open and each change arrives as one
stream item. Served bytes are pinned in the
[watch corpus](../../test-vectors/consent-watch-pending-v1/README.md).

**Admission** is exactly that of `consent.list_pending`: an open store
(`consent_unavailable` otherwise), then the phone's operator route
(`elicitation_operator_required` for every other caller, including Callosum's
Mac and simulator routes), then a well-formed `clientFeatures`. Called as an
ordinary unary request, it is refused after that admission with
`elicitation_invalid_request`. While ck-cingulate is stopping, a new watch is
refused `consent_unavailable`. At most 16 watches are open per process; one more
is refused `watch_capacity` (`at most 16 watches may be open`).

**Request** params are `{cursor?, clientFeatures?}`. `clientFeatures` has the
same meaning as on `consent.list_pending` and `consent.get`.

**Items** are JSON objects, each at most 192 KiB encoded:

| Item | When |
|---|---|
| `{"type":"reread"}` | First item when the request has no usable cursor; also replaces an item that would exceed 192 KiB, and closes a watch whose reader fell too far behind. The phone reads `consent.list_pending` once. |
| `{"type":"changed","cursor":C,"card":Card}` | A phone-visible card was created, answered, withdrawn, expired or settled (an execution report), or a permission ask changed. `card` is byte for byte the phone `consent.get` projection of that card with the watch's `clientFeatures`, package bodies included. |
| `{"type":"removed","cursor":C,"elicitation_id":Id}` | The card is no longer readable by phone `consent.get`: retention pruned it, or it is a permission ask that never reached the user. |

There is no snapshot message. A usable cursor is one this store minted, not
older than its retained change log and not ahead of it. A missing, malformed,
foreign, future or too-old cursor gets `reread` as the first item and is not an
error.

**Cursor** `C` is an object `{"instance": String, "version": Integer}`. The
phone stores the cursor of the last item it applied and echoes it back
unchanged; it must not construct or interpret it. `instance` names the store
file, and `version` increases with every item, so applying an item twice is
harmless: keep, per card, the item with the highest version. A watch opened
with a usable cursor replays every card that changed after it, one item per
card at that card's latest change, in version order, and then continues live.
Every change committed after the watch read its starting position is streamed,
and the phone's list read follows the `reread` item, so no change falls
between the two.

**Ending.** The stream ends cleanly (StreamEnd, no error) when the caller
unsubscribes, the route closes, or ck-cingulate starts draining or loses its
daemon connection; none of these sends a final item, and after a restart the
phone's last cursor still replays. Refusals come only before the first item.
Once an item has been sent, an internal error (a store failure, or a card
that cannot be projected) ends the stream with a final `reread` and a clean
end, never an error reply. A watch keeps only its position in the store's change log,
never a queue of changes. If more than 256 cards changed after that position
while the reader was not keeping up, the watch sends `reread` and ends; the
phone reconnects. The store keeps the newest 4096 change-log rows; a cursor
older than those gets `reread`.

Never infer an answer or a grant from a watch item's absence, a `removed` item,
a `reread`, or the stream ending: those only say what to read again.

## Rich UI provider approvals (0.11.0)

Cingulate advertises `rich-ui-approval/v1`. To see `kind: "rich_ui_approval"`,
every phone list, get, watch and answer call must declare
`clientFeatures: ["rich-ui-approval"]`. Without it, lists and watch streams omit
these cards (including removal IDs), get refuses `elicitation_not_found`, and an
answer on the intended device refuses `elicitation_rich_ui_approval_required`.
This gate prevents older phones from encountering the new `revoked` state.

The daemon-stamped route must be `reserved:callosum` with a phone harness
`fed-phone:<32 lowercase hex fingerprint>`. A card is visible only on the phone
whose fingerprint equals its `richUiApproval.device`. Malformed harnesses and
other devices see no card: get returns the unknown-ID refusal; answer returns
`{ok:false, code:"not_found"}` exactly as for an unknown ID, even when features or
binding are absent. Request body fields never establish route identity.

The phone card carries the typed nested body:

```json
{"richUiApproval":{"approvalDigest":"sha256:<64 lowercase hex>","manifestDigest":"sha256:<64 lowercase hex>","capabilityDigest":"sha256:<64 lowercase hex>","provider":"module-id","device":"<32 lowercase hex>"}}
```

The phone verifies its manifest and computes the digest itself, and renders its
own approval text. Cingulate checks field formats, not manifest contents or digest
recomputation. Options contain exactly one grant and one decline; the default is
the decline and `onExpiry` is `deny`. Silence, timeouts and errors never approve.
No new-card or retraction notifier pointer is sent for this kind.

`consent.answer` adds optional `bindDigest`. A grant requires it to equal
`richUiApproval.approvalDigest` exactly; a decline may omit it, but a supplied
value must match. Missing grant bindings, nulls and different values refuse
`approval_digest_mismatch` without recording an answer. Supplying `bindDigest`
on any other kind refuses `invalid_params` naming the field.

A grant or decline has state `answered` and the selected `answeredChoiceId`.
The requester can later revoke a grant: phone get and feature-enabled watch
changed items then report `state: "revoked"` and `revokedAtMs`. It never returns to
pending. Normal withdrawal behavior and 24-hour settled-card retention remain.
An accepted generic-card answer from a parsed fed route records `answeredDevice`
for audit; permission asks record the fingerprint on their stored answer attempt.
The fingerprint is never taken from answer params.

Complete served examples and error envelopes are pinned in
[rich-ui-approval-v1](../../test-vectors/rich-ui-approval-v1/README.md).

## Vault secret-use approvals (0.12.0)

Cingulate advertises `secret-use-approval/v1`. Only daemon-stamped
`reserved:callosum` routes with harness `fed-phone:<32 lowercase hex>` and
`clientFeatures: ["secret-use-approval"]` may read or answer `kind: "secret_use"`.
List, get and watch hide these cards from older phones and all other devices;
watch also hides removal ids after payload pruning. Both allow and decline
require the feature; missing it refuses `elicitation_secret_use_approval_required`.
Get without the feature refuses `elicitation_not_found`. Agents and computers
are never answering surfaces for this kind.

The flat phone record carries `secretUse: {items: [{request_id,
statement_b64url}]}`. Nested fields keep the vault's snake_case spelling. Decode
each canonical statement and render its agent, secret, scope, coverage, use,
offered durations and request deadline. The subject label comes from Cingulate's
route-stamped agent resolution. Generic title, prompt and allow/decline labels
are fixed by Cingulate, never supplied by the filer. The default is decline and
`onExpiry` is deny; the earliest `request_expires_at_ms` is `expiresAtMs`.

Decline uses `choiceId: "decline"` with no envelope. For allow, first send the
signed envelope to the vault's `secret.approve` with `card_ref` equal to this
`elicitationId`; then answer Cingulate with `choiceId: "allow"` and the exact
`envelope_b64url` sent to the vault. Cingulate checks, without verifying the
signature:

- strict unpadded base64url, RFC 8785 canonical JSON and exactly
  `{v, items, decision, duration, grant_expires_at_ms, device_fingerprint,
  issued_at_ms}`; `v` is `ck-secret-use-envelope/v1`, `decision` is `allow`;
- every integer is nonnegative and at most 2^53 − 1, with no floats;
- `device_fingerprint` equals the answering route's fingerprint;
- 1–16 items with closed shape `{request_id, statement_sha256}`, no duplicates,
  each naming a card item and the lowercase SHA-256 of its decoded statement;
- `duration` is offered by every listed statement.

Malformed or mismatched bindings refuse `elicitation_invalid_request` without
recording an answer. Envelope params on kinds other than `secret_use` or secret-bearing `flow_install` refuse `invalid_params`.
Answers at or after expiry refuse `elicitation_expired`, including declines and
same-choice retries. The vault separately checks its signature, live requests,
grant expiry arithmetic, pairing and current sealed-secret incarnation.

Accepted answers expose `secretUseAnswer` containing `decision`, optional
`duration`, all card `items: [{request_id, decision}]`, and optional
`envelope_sha256`. Items left out of an allow envelope count as declined.
A whole decline omits duration/hash. The hash is over the decoded exact envelope
bytes and matches the vault grant/audit hash, not a reconstructed decision.
Repeating an allow with a different binding conflicts rather than replacing
the recorded decision. [Single and batch served records](../../test-vectors/secret-use-approval-v1/README.md)
are generated through the real phone get path and pinned byte for byte.


## Flow-install secret approvals (0.13.0)

`flow-install-secrets/v1` lets one install card approve a flow and its vault
secrets together. Phone projections carry `flowInstall.flow_identity` and
`flowInstall.secrets: [{request_id, statement_b64url}]`; nested keys and statement
bytes are unchanged from core's filing. `flowInstall.flow` is still the existing
flow name. Decode and show the vault statements verbatim alongside the install
card: the identity is the installed flow version, not an agent scope. The
statement domain is `ck-secret-flow-statement/v1`, with only `version` offered.
The [requester contract](consent-requester-v2.md#flow-install-secret-approvals-0130)
describes the closed shapes and filing refusals.

The same `secret_use` gate applies, with no new phone feature name:
`reserved:callosum`, harness `fed-phone:<32 lowercase hex>`, and
`clientFeatures: ["secret-use-approval"]`. Unsupported phones, computers,
simulators and agents cannot read or answer, even to decline. List/get/watch
and pruned watch-removal ids are filtered identically. Plain `flow_install`
cards remain visible and answerable exactly as before.

For an install/grant choice, sign one `ck-secret-use-envelope/v1`, submit it to
the vault with Face ID as for `secret_use`, then answer the card with the same
`envelope_b64url`. Use the card's grant option id as `choiceId`; it need not be
`allow`. The envelope must have `decision: "allow"`, `duration: "version"`,
`grant_expires_at_ms: 0`, and the answering phone's fingerprint. All existing
canonical, closed-shape, integer and statement-digest envelope checks apply.
Items must be **exactly every** card `{request_id, statement_sha256}` pair, each
once, in any order. Subsets, extras, duplicate items and another phone's envelope
are refused without recording anything. Unlike agent secret-use approvals,
partial approval cannot install a flow. A decline option needs no envelope.

`expiresAtMs` is the earlier of the filed expiry (if present) and the earliest
statement expiry. No filed expiry is normal; statements can live up to 24 hours.
On deadline the card expires denied, never installs, regardless of filer policy.
Answers and retries at or after deadline refuse `elicitation_expired`.
`secretUseAnswer` records duration `version`, all item decisions and the exact
envelope hash; `answeredDevice` records the phone fingerprint. Same choice and
same bytes return the recorded answer; any other valid binding or choice
conflicts. Cingulate does not verify the signature or release secrets: the vault
does. [Generated filing, phone, answer and refusal vectors](../../test-vectors/flow-install-secrets-v1/README.md)
pin the contract without changing older corpora.

## Forwarded secret statements v2 (0.13.1)

`secret_use` and secret-bearing `flow_install` cards may carry v2 statements:
the v1 statement plus `asking_member_id` (`fed-computer:` and 32 lowercase hex
characters), with `v` set to `ck-secret-use-statement/v2` or
`ck-secret-flow-statement/v2`. They mean an agent or flow on that paired
computer asked, and the home vault named by `member_id` will serve the grant.
Statement bytes reach the phone unchanged, and answering is identical: the same
envelope, with each item's `statement_sha256` over the exact statement bytes.
Cingulate accepts a card only when all its statements share one asking computer
(or none has one) that differs from `member_id`. [secret_use](../../test-vectors/secret-use-approval-v2/README.md)
and [flow install](../../test-vectors/flow-install-secrets-v2/README.md) cards
are generated from the vault's own v2 vectors and pinned byte for byte.
