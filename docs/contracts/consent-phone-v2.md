# Phone consent contract, v2

Version 2 includes the [v1 interface](consent-phone-v1.md) unchanged for existing
kinds, plus `tool_permission`, `macos_permission` and the feature-gated vault
kinds `secret_use` and `manifest_binding` described below. It is a new version rather
than claiming additive compatibility: these kinds have no lifetime
fields, and late same-choice answers conflict. Clients pinned to v1 must not
render unknown kinds as ordinary expiring approvals. The shared
[served vectors](../../test-vectors/tool-permission-v1.1/README.md) pin both phone
and requester bytes. The [requester v2 contract](consent-requester-v2.md) specifies
admission, identity, state machine and core notices.

Package and flow-install get/list cards also support the optional plain-text
[`explanation` and `changes` objects](consent-card-explanations-v1.md), including
core-supplied labels. Omitted members stay omitted; push notices carry neither.
The linked contract includes caps, character rules and shared desktop samples.

`secret_use` has a hard deadline: both allow and decline are refused at or after
the earliest statement expiry, even if the same choice was already recorded.
Late-answer rules never extend a vault request's lifetime.

Generic consent also admits worker sessions without their own agent id, as
specified below. Cards raised by other sessions and existing served vectors
remain unchanged.

## Structured flow-card content

Since ck-cingulate 0.25.3, `package` and `flow_install` cards may also carry
`warnings`, `comparison` and `capabilities` on phone and desktop list/get/answer
records and changed-card watch items. Their names and nested keys are unchanged
on every projection. Absent members are omitted, never null; pointer and
settlement notices do not carry them. Package lists keep these members even
when script and manifest bodies are omitted.

All strings are nonempty plain text with these UTF-8 byte bounds:

| Member | Shape and bounds |
|---|---|
| `warnings` | 1–4 strings, each 1–300 bytes |
| `comparison` | `{against, note?, rows}`: `against` 1–120 bytes, `note` 1–1,200 bytes, 1–24 rows |
| Comparison row | `{label, change, before?, after?}`: `label` 1–64 bytes, `change` 1–32 bytes, `before` and `after` each 1–1,200 bytes |
| `capabilities` | `{rows}`: 1–24 rows of `{label, value}`, `label` 1–64 bytes, `value` 1–1,200 bytes |

All object member sets are closed. `change` permits only lowercase ASCII letters
and underscores, but the token set is open. `changed` requires `after` and
optionally includes `before`; `added` requires `after` without `before`;
`removed` requires `before` without `after`. Unknown values require at least
one of `before`/`after`: readers draw unknown change values as plain rows.
Warnings, `against`, row labels and `change` reject all C0/C1 controls, including
newline. `note`, `before`, `after` and capability `value` permit newline but no
other C0/C1 controls, like author text. Limits are byte counts, not character
counts. Invalid shapes, nulls, bounds or change rules are refused with fixed
`elicitation_invalid_request` messages. Other kinds refuse these members;
`secret_use`, `config_plan` and `manifest_binding` do so before normalization.

Draw warnings immediately below the question. Comparison rows can show the old
value struck through followed by the new value; a changed row without `before`
shows only its new value. Capability rows describe new installs. All words come
from the producer; render plain text rather than interpreting markup or treating
claims as verified. Readers without support keep showing `changes`. Producers
retain that fallback until the new readers ship.

These members are stored unchanged as card content, including for deduplication:
changed content replaces a matching pending `dedup_key`, identical content
refreshes it. The [requester contract](consent-requester-v1.md#optional-structured-flow-card-content)
and [served samples](../../test-vectors/flow-card-structure-v1/README.md) cover the
same data on requester, phone and desktop surfaces.

## Identity and visibility

Federated generic-consent cards carry `subject.federated` (nested protocol keys
unchanged), no local agent id, and a computer-first `prompt` built by Cingulate.
Render that prompt and label as supplied; do not resolve the remote ids as local
agents. The exact claim wording, paired-computer fact, run single-use behavior
and route-independent lifecycle are in
[federated consent carriers](consent-requester-v2.md#federated-consent-carriers).
For a federated Callosum operator route, the stamped peer class must be `phone`;
an unstamped route continues to use the harness rule below.

The default operator rule accepts either a daemon-stamped `operator_app` desktop
route or `reserved:callosum` on a daemon-stamped harness starting `fed-phone:`.
No body field establishes authority. Permission
asks appear in `consent.list_pending` **only** while in `pending:user`, projected
as `state: "pending"`. Phone `consent.get` continues to serve a permission ask
after settlement if its durable surface history includes the user leg. A worker's
ask that never reached the user leg refuses `elicitation_not_found`, both before
and after settlement. Removing a card from pending lists does not hide its final
phone get projection. Existing generic consent kinds follow the v1 rule: phone get
continues to return a settled card after requester acknowledgment until the
24-hour retention prune removes its payload.

For development installations, the default-off `dev-local-operator` Cargo
feature also admits daemon-stamped
`Principal::Direct` programs as operator surfaces for listing, fetching, answering
and watching ordinary cards. Ordinary local answers have no device fingerprint; permission
answer audits retain the real principal `direct`. Secret-use, manifest-binding,
and secret-bearing flow-install cards have the development exception described
in [vault secret-use approvals](#vault-secret-use-approvals-0120); config-plan
cards remain hidden and unanswerable on this route. Public release builds must omit the
feature; the `DEVELOPMENT_LOCAL_OPERATOR` environment variable has no effect.
Version output and health detail expose the feature's build-time state.

Local programs also retain their requester operations. A local `consent.get` uses
the operator projection only when `elicitationId` is present; `elicitation_id`
selects the requester projection and its ownership checks. Phone dispatch is
unchanged.

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
Display these as untrusted data. Cingulate sets the short `title` to
"Worker permission" for a stamped worker scope and "Agent permission" otherwise.
The `prompt` headline names the asking agent when its display name is available
and uses "An agent" otherwise. The provider's `requesterName` is not a stand-in
for the agent's name. Tool cards have no `target` or expiry field.
`options` are exactly `allow_once` and `allow_scope` (effect `grant`) and
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
| Non-phone caller | Transport refusal `elicitation_operator_required` (except `reserved:prefrontal` relays, whose parent identity and leg are checked separately). |

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

Answers are attributed for audit. When the answer arrives on a fed route (the
daemon-stamped principal is `reserved:callosum` and the harness is
`fed-<class>:<fingerprint>`, where the class is `phone`, `sim`, `computer` or the
older `mac`, with a 32-character lowercase hex fingerprint), an accepted
generic-card answer records that
fingerprint as `answeredDevice`, and a permission ask records it on its stored
answer attempt. The fingerprint is never taken from answer params, and it is an
audit record only: whether the caller may answer is decided separately, by the
operator route rules above.

An accepted ordinary desktop answer adds `answeredSurface: "operator_app"` to
the card (`answered_surface` in requester and outcome records). It is optional
and omitted for phone and development-local answers, including historical
records. `answeredDevice` remains the phone fingerprint and is not repurposed.
Permission answer attempts retain `surface: "user"` and final
`answered_by: "user"`, with optional audit provenance `via: "operator_app"`.
These fields come only from the daemon's bind stamp, never from answer params.

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

## Manifest-binding approvals (0.18.0)

**Readers first.** Phone and desktop renderers must implement this kind before
Plexus files a card. Cingulate advertises `manifest-binding-approval/v1`.
A reader advertises `clientFeatures: ["manifest-binding-approval"]` only once it
can independently decode the statement and render this kind. The older
`secret-use-approval` feature is not sufficient. In release builds, only a daemon-stamped
`reserved:callosum` phone route (`fed-phone:<32 lowercase hex>`, or the equivalent
daemon-stamped federated phone identity) with this feature may read or answer.
List/get/watch hide cards and pruned removal ids from unsupported readers and
other devices. Unsupported get refuses `elicitation_not_found`; unsupported
answer refuses `elicitation_manifest_binding_approval_required`. The [development-build exception](#vault-secret-use-approvals-0120) also applies to manifest-binding cards.

The flat card has `kind: "manifest_binding"` and the read-only payload:

```text
manifestBinding: {
  statement_b64url, statement_sha256, agent_id, app_slug, repo,
  installation_permissions: {<permission>: "read" | "write" | "admin", ...}
}
```

Nested names retain their snake_case spelling. Decode the canonical statement
yourself, check its SHA-256 against `statement_sha256`, and refuse a card whose
served facts or `expiresAtMs` disagree with those bytes. Draw and sign only from
that independent decode. Render the repository exactly (including casing), the
bot `app_slug`, the full permissions map, and the binding's `agent_id`. Resolve
the agent's display name from core's agent record, not Cingulate or filer text.
The statement has exactly the keys and version described in the
[requester contract](consent-requester-v2.md#manifest-binding-requester-0180).
The default is decline, `onExpiry` is deny, and expiry is the statement's
`expires_at_ms` with no extension. All generic labels are fixed by Cingulate.

Decline sends `consent.answer` with `{elicitationId, choiceId:"decline",
clientFeatures:["manifest-binding-approval"]}` and no envelope. Send nothing to
the vault. For allow, sign the exact JCS envelope and send `{envelope_b64url,
signature_b64url}` straight to the vault's `manifest.approve`. After the vault
accepts, answer Cingulate with `{elicitationId, choiceId:"allow",
clientFeatures:["manifest-binding-approval"], envelope_b64url}` using those
same envelope bytes. Cingulate does not call the vault or verify signatures.

The envelope's closed shape is `{v, proposal_id, statement_sha256, decision,
device_fingerprint, issued_at_ms}`. Cingulate requires unpadded base64url and JCS,
`v == "ck-manifest-binding-envelope/v1"`, `decision == "allow"`, the stored
proposal id and exact statement digest, and a lowercase-hex device fingerprint
equal to the answering phone route's. Integers must be nonnegative and no larger
than 2^53 − 1. Malformed or mismatched fields refuse
`elicitation_invalid_request`. As for secret-use approvals, the vault owns the
signature and issued-at freshness checks. Cingulate refuses all answers at or
after card expiry with `elicitation_expired`, including declines and retries.

The served `manifestBindingAnswer` contains `decision`, `statement_sha256`, and
for allow only `envelope_sha256`, hashing the exact decoded envelope. The
route-derived fingerprint is `answeredDevice` beside the answer. Expired cards
have no answer object. Same-choice retries require the same decision binding;
they never replace a recorded decision with another envelope. Requester
get/await use `manifest_binding_answer` instead, as described in requester v2.

The [samples](../../test-vectors/manifest-binding-v1/README.md) pin allow, decline,
expired and mismatch-refusal cases. `phone-refuse-mismatch.json` deliberately
mutates a decoded fact on a real card: it is an input the phone MUST refuse,
not a card Cingulate serves. The vault samples and contract come from claustrum
tag `manifest-binding-vectors-v1`, commit
`c2a9ce2eaf99da646160eeb0c73d6b11d9341cbf`.

Escalate, settle and withdraw pushes carry only Cingulate-vouched metadata,
never repository, statement or permissions; fetch the card on the admitted route.

## Broker-action approvals (0.27.0)

See the [broker-action contract](broker-action-v1.md#rendering-and-answering)
for the `broker-action-approval` feature, render and answer rules, and the
24-hour retention of missed cards.

## Vault secret-use approvals (0.12.0)

Cingulate advertises `secret-use-approval/v1`. In release builds, only daemon-stamped
`reserved:callosum` routes with harness `fed-phone:<32 lowercase hex>` and
`clientFeatures: ["secret-use-approval"]` may read or answer `kind: "secret_use"`.
List, get and watch hide these cards from older phones and all other devices;
watch also hides removal ids after payload pruning. Both allow and decline
require the feature; missing it refuses `elicitation_secret_use_approval_required`.
Get without the feature refuses `elicitation_not_found`. Agents and computers
are not answering surfaces for this kind in release builds.

In development builds with `dev-local-operator`, the daemon-stamped local-operator
`direct` route used by the desktop app may read and answer `secret_use`,
`manifest_binding`, and secret-bearing `flow_install` cards with the respective
client feature. Its device fingerprint is `00000000000000000000000000000000`;
the envelope is the phone's `ck-secret-use-envelope/v1` or manifest-binding
approval envelope with that fingerprint. Top-level `signature` and
`signature_b64url` members are refused. Release builds omit this feature and
retain the phone-only rule. The [desktop development samples](../../test-vectors/desktop-dev-cards-v1/README.md)
pin the served cards and answers.

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
recording an answer. Envelope params on kinds other than `secret_use`,
`manifest_binding`, `broker_action` or secret-bearing `flow_install` refuse
`invalid_params`.
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

The same release-build `secret_use` gate applies, with no new phone feature name:
`reserved:callosum`, harness `fed-phone:<32 lowercase hex>`, and
`clientFeatures: ["secret-use-approval"]`. Unsupported phones, computers,
simulators and agents cannot read or answer, even to decline. List/get/watch
and pruned watch-removal ids are filtered identically. Plain `flow_install`
cards remain visible and answerable exactly as before. The
[development-build exception](#vault-secret-use-approvals-0120) applies to secret-bearing cards.

For an install/grant choice, sign one `ck-secret-use-envelope/v1`, submit it to
the vault with the phone's user-presence check (biometric or device passcode) as for `secret_use`, then answer the card with the same
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

## Extended secret statements v3 (ck-cingulate 0.16.2)

The statement domains `ck-secret-use-statement/v3` and
`ck-secret-flow-statement/v3` carry extended scopes for local and forwarded
requests. `asking_member_id` is absent locally and present for forwarded requests,
with the same grammar and serving-vault exclusion as v2. Legacy scopes still use
v1 locally and v2 when forwarded.

For v3, `agent_api_key` carries `use: {rules: [{host, path_prefix}, ...], location}`;
flow API keys also carry sorted `methods` beside `rules`. Each rule has both
string keys, with `""` for no prefix. `site_login` carries `use: {origins: [...]}`.
Cingulate refuses kind/shape or version/shape mismatches and unknown or mixed
fields, but the vault owns host grammar, public-suffix policy and scope matching.

Render rules and origins in their canonical signed order. Show all hosts and
prefixes, or the first few plus "and N more"; preserve wildcard spelling such as
`*.x.test`. Never display the first entry as though it were the entire scope.
Cingulate supplies no scope-derived text or labels: it relays the statement bytes
unchanged, so scope rendering remains the phone's responsibility.

Approval envelopes, statement digests, signatures and the recorded
`envelope_sha256` are unchanged. See the
[pinned v3 samples](../../test-vectors/secret-statements-v3/README.md) and the
[requester contract](consent-requester-v2.md#extended-secret-statements-v3-ck-cingulate-0162)
for closed shapes and reserved flow-id tags.

## Config-plan approvals (0.21.0)

**Readers first:** CKIOS must read the checksum-pinned
[config-plan v3 samples](../../test-vectors/config-plan-v3/README.md) and render
all new members. Callosum files the new typed program-change flags, rollback
program detail and `route_rebind` transition only after the phone's reader is
live; until then Callosum keeps filing them as other sections (Decline-only). Declare `clientFeatures: ["config-plan"]` on list/get/answer.
Cingulate serves these cards only to daemon-attested Callosum phone routes;
local desktop operator authority, simulators and older readers do not qualify.
Without opt-in, lists omit the card, get refuses `elicitation_not_found`, and
answer refuses `elicitation_config_plan_required` (including a decline).

The card subject and “who asks” line are **This computer's settings**, never an
agent, even if the requester route carries an agent scope. The phone renders all
recorded `old`/`new` JSON values and sources, and treats a field row's `apply` as an open string
(known: `live`, `module_restart`, `daemon_restart`, `next_supervision`,
`route_rebind`). Unknown field apply modes and `other_sections` remain visible as raw
values, not silently omitted. Nonempty `other_sections` makes the card Decline-only.
The plan-level `apply` object is distinct from the field string and has a closed
transition enum. Nested plan keys retain snake_case; only the
record's top-level `config_plan` becomes `configPlan`. The exact pending get
result for the invented sample is:

```json
{"answerSeq":0,"configPlan":{"apply":{"operation":"change","rollback_to":null,"modules":[],"review":null},"base_revision":"42","changes":[{"apply":"live","module_id":"example-module","new":true,"new_source":"project","old":false,"old_source":"default","path":"settings.enabled","project_id":"example-project"}],"other_sections":{"policy_changes":[{"example":true}]},"plan_digest":"fake-plan-digest"},"createdAtMs":10000,"default":"decline","elicitationId":"el_1","expiresAtMs":310000,"expiresInMs":300000,"facts":[],"kind":"config_plan","lateExecution":"notify_only","materialDamage":false,"onExpiry":"deny","options":[{"effect":"choose","id":"approve","label":"Approve"},{"effect":"decline","id":"decline","label":"Decline"}],"prompt":"Review this computer's settings changes.","requesterLabel":"reserved:callosum","requesterName":"This computer's settings","scope":{},"state":"pending","subject":{"label":"This computer's settings"},"target":{"kind":"config_plan","label":"This computer's settings"},"title":"Settings change approval","urgency":"normal"}
```

All section objects and rows are closed. The decoder refuses unknown members,
missing required members, invalid types/enums, duplicate ids and exceeded caps
as `elicitation_invalid_request` at filing, never truncating a section.

| `configPlan` member | Shape and filing caps |
|---|---|
| `plan_digest`, `base_revision` | Digest is 1–128 UTF-8 bytes; revision is a nonempty ASCII decimal string. |
| `changes` | Required 0–64 rows: `module_id`, optional `project_id` (each 1–128 UTF-8 bytes), `path` (1–1,024), `old`, `new`, `old_source`, `new_source`, `apply` (the last three strings each 1–64). Values are recorded JSON, including null/redaction, at most 4,096 serialized UTF-8 bytes each. |
| `apply` | Required `{operation, rollback_to, modules, review, rollback_programs?}`. Operation is `change` or `rollback`; `rollback_to` is a nonempty ASCII decimal string exactly for rollback, otherwise null. |
| `apply.modules` | 0–64 rows with required unique `module_id` (1–128 UTF-8 bytes), `transition` (`live`, `module_restart`, `route_rebind`, `next_start`, `daemon_restart`), booleans `whole_generation`, `waits_for_next_start`, `waits_for_daemon_restart`, `deferred_live_fields` (0–64 paths, each 1–1,024 UTF-8 bytes), and plain-text `explanation` (1–512 UTF-8 bytes). |
| `apply.review` | Required null or closed `{"supervisor_reconcile":true}`; false is refused. |
| `apply.rollback_programs` | Optional 0–64 closed rows `{module_id, input, approved_then, on_disk_now}`, allowed only for `operation: "rollback"` (even an empty array is refused on change). Module id is 1–128 UTF-8 bytes, input 1–1,024; both values are required recorded JSON including null/redaction, at most 4,096 serialized UTF-8 bytes each, as for change-row `old`/`new`. No `reason`: Callosum drops the daemon's reason before filing. |
| `affected_projects` | Optional 0–64 unique ids, each 1–128 UTF-8 bytes. |
| `policy_changes` | Optional 0–64 rows `{kind, module_id?, layer?, old, new, reason?}`. Kind is `declaration`, `subc_jsonc`, `reviewed_program` or `layer_file`. Module id (1–128 UTF-8 bytes) is required except for `subc_jsonc`, where it must be absent. |
| policy values/reason | Non-`layer_file` values use the same JSON/redaction form and 4,096-byte cap as `changes`; optional reason is at most 512 UTF-8 bytes. |
| `layer_file` policy | Required `layer` (1–64 UTF-8 bytes), allowed only on this kind. Each value is a closed `{"absent":true}` or `{"sha256":"<64 lowercase hex>","bytes":<non-negative integer>}` object. No `reason`. |
| `launch_manifests` | Optional 0–64 closed entries `{module_id, origin?, programs, program_changed_since_target?, program_changed_since_active?}`, unique by module id (1–128 UTF-8 bytes). Optional origin is 1–64 UTF-8 bytes when present, never null. Required programs is 0–16 rows `{input, old, new}`; input is 1–1,024 UTF-8 bytes; values use the same JSON/redaction form and 4,096-byte cap as `changes`. Each entry needs at least one program or flag; flag-only entries carry `programs: []` and may omit origin. |
| manifest `program_changed_since_target`, `program_changed_since_active` | Optional flags, present only as literal `true`. False, null and every other value refuse. Either or both mean the module restarts on the plan's program. |
| `other_sections` | Optional raw JSON object, at most 16,384 serialized UTF-8 bytes; present null is invalid. Nonempty means Decline-only. |

`apply` shows when the change takes effect, including restarts and reconciliation.
Its explanation is literal plain text, not HTML or Markdown; whitespace is
preserved and controls U+0000–U+001F and U+007F–U+009F other than U+000A newline
are refused, as on other card explanations.
`affected_projects` names the impacted projects. `policy_changes` shows changes
to authority; `subc_jsonc` identifies the daemon's subc.jsonc policy configuration.
Word `layer_file` rows as **settings file changed; no setting changed**
because comments, formatting, same-value replacements, file creation or removal
can leave effective settings unchanged. `launch_manifests` shows changes to
programs that will run. Show `program_changed_since_target` as "Its program changed
since that version. It restarts on the program now on disk." Show
`program_changed_since_active` as "It is running an older program. It restarts on
this plan's program." `apply.rollback_programs` compares the program approved
then with the program now on disk to show which program will run on rollback.
For `route_rebind`, the module keeps running and the change counts as active only
after its connections reopen. Show "This module's open connections close and
reopen under the new settings." `next_supervision` is not a module transition;
Callosum maps the daemon's value to `next_start`.
`changes` shows field values and sources, and
`other_sections` preserves information the reader cannot yet render safely.
Approval requires rendering all these sections, not just field rows.

At least one row is required across `changes`, `policy_changes` and
`launch_manifests`, whose entries each need a program or flag. A program-only,
flag-only or `layer_file`-only plan can therefore have zero field rows. Restart
scope, rollback program detail or affected projects alone is not a plan. Optional lists and optional row strings cannot be present null. `binding`
remains a private requester object capped at 4,096 serialized UTF-8 bytes and is
never sent to the phone; the existing frame bound is 1,310,720 bytes.
Legacy stored cards missing `apply` remain readable but are projected Decline-only;
an approval attempt refuses `elicitation_invalid_choice`.
All sections survive filing and round reopening unchanged in phone get/list/watch
and requester get/await, with nested snake_case names preserved.

Pending list records use the same projection. Phone list/get/answer readbacks
**never include the requester's opaque `binding`**. The options and default are
fixed: approve (`choose`) or decline (`decline`, default); expiry is denial.
Before asking for the user-presence check (biometric or device passcode), the phone must check the plan digest of
what it rendered, then sign the config-consent statement with its consent key.
Callosum owns that statement's grammar, key registration and verification.
Cingulate does not parse or verify it. The following intentionally fake bytes
show only Cingulate's transport shape, not a valid consent statement/signature:

```json
{"elicitationId":"el_1","choiceId":"approve","clientFeatures":["config-plan"],"statement":"_wABeCBub3QtSlNPTgo","signature_b64url":"AQIDBA"}
```

Include `expectedRound` with the non-negative integer round the person reviewed;
omission means 0, the first round. This member is camelCase like `elicitationId`.
Cingulate checks the round inside the transaction that records the answer,
before exact-retry and replay checks. A mismatch returns a typed answer result,
for example `{"ok":false,"code":"round_changed","detail":{"currentRound":1}}`.
Nothing is recorded. Reread the current card and ask for a new decision; never
silently resubmit a stale decision with the newer round number.

An exact retry of the current round's recorded choice and signed strings stays
idempotent (`alreadyAnswered: true`). Any other answer whose decoded statement
SHA-256 was already recorded on this card, in any round or choice, returns
`{"ok":false,"code":"statement_reused"}` without recording it. Signing the same
statement again does not make it fresh. A reopened round requires a new decision,
fresh statement bytes and a matching `expectedRound`; an omitted round on a
reopened card returns `round_changed` first. Both refusals are typed answer
results, not transport errors. Other card kinds ignore `expectedRound` if
present, just as they ignore unrelated unknown answer params.

Approve without either signed field refuses `elicitation_invalid_request`.
`statement` must be nonempty, strict unpadded base64url with at most 4,096 decoded
bytes; `signature_b64url` has the same encoding and at most 1,024 decoded bytes.
Cingulate preserves both exact strings, without parsing signed content or
canonicalizing it, and returns them unchanged to Callosum on get/await.
Do not use `envelope_b64url` on this kind. A decline can omit the signed fields:

```json
{"elicitationId":"el_1","choiceId":"decline","clientFeatures":["config-plan"]}
```

Accepted answers use the existing `ok: true`, `alreadyAnswered: false` wrapper
with the settled `elicitation`; it adds `answeredChoiceId`, `answeredAtMs`,
`answeredDevice`, `answerSeq: 1`, `statement` and `signatureB64url` (if supplied).
Repeating exactly the same answer in the current round returns `alreadyAnswered: true`;
a different answer to a settled card conflicts unless its statement digest is
reused, which returns `statement_reused` first. A Cingulate accepted approval is **not** a
verified or applied plan: Callosum alone decides that.

When Callosum reports failed verification while time remains, the same card
reopens. Its old choice, answer time/device and signed bytes disappear; its
`answerSeq` remains 1 and its phone projection includes exactly:

```json
{"round":1,"unverified":{"code":"signature","reported_at_ms":10000}}
```

Possible codes are `time`, `plan`, `decision`, `signature`, `key`. Show **The
approval couldn't be verified. Nothing was changed.**, not “Declined”. Permit a
new signed answer within the remaining original lifetime. The next answer uses
`answerSeq: 2` and clears `unverified`. Reopening bumps the pending revision and
pushes the card again. Withdrawal and expiry retract it; no timeout or missing
signature grants anything. Retained settled cards are readable for 24 hours.

The card record in `consent.list_pending`, `consent.watch_pending` changed items
and `consent.get` includes `round`, the JSON integer reopen count, with the same
value as that round's person notices. It is absent on the initial round, never
emitted as 0, and remains present after that round settles. A higher `round`
supersedes a final copy of the same elicitation id and shows as pending again.
Do not treat an id as permanently final across higher rounds. Requester
get/await records carry the same `round`; it is not renamed to `noticeRound`.
