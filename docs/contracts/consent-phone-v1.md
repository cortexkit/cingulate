# Phone consent contract, v1

Permission asks require [v2](consent-phone-v2.md). The v1 shapes and pinned
examples below remain unchanged for their existing kinds.

This contract describes the implemented approval-card interface. The phone calls
exactly `consent.list_pending`, `consent.get`, and `consent.answer` through
Callosum. Requests are JSON envelopes:

The additive `source` and `dryRunText` card fields are specified in
[consent-card-text-v1](consent-card-text-v1.md).

Package and flow-install cards may also carry `explanation: {label, text}` and
`changes: {label, text}` on `consent.get` and every `consent.list_pending` entry.
These plain author texts and core-supplied labels are returned unchanged;
absent members are omitted. Labels are 1–64 Unicode scalar values and text is
1–8,192 UTF-8 bytes; C0/C1 controls are refused except newline. Notifications
omit both objects. See [consent-card-explanations-v1](consent-card-explanations-v1.md)
for the capability, precise validation and shared phone/desktop samples.

```json
{"method":"consent.get","params":{"elicitationId":"el_1"}}
```

Successful protocol responses are JSON envelopes with one `result` member.
Shapes below describe that member, not an additional nested envelope. Handler
refusals are transport errors carrying a string `code` and `message`, **not**
`result` objects. Branch on codes, not message wording. An answer result with
`ok: false` is a normal protocol response, not a transport refusal.

The [served examples](../../test-vectors/consent-phone-v1/README.md) preserve the
actual response bytes, including the envelope. Their regeneration and integrity
tests check all bytes and hashes. The existing record corpora are unchanged.

## Identity and admission

The daemon's route bind supplies identity. A phone operator route requires both
principal `reserved:callosum` and a bind harness beginning with the literal
`fed-phone:` prefix. The phone must not attempt to supply either in `params`.
A session, agent, device or machine identifier in the body does not establish
authority. A phone route may list and fetch all operator-visible cards; these
three operations have no subject, agent or device filter.

For **all three phone operations**, unrecognized params are ignored, regardless
of their type. In particular, all the following identity-looking params are
ignored, not validated and not used to authorize or filter:

| Identity a client might try to supply | Ignored param spellings |
|---|---|
| Requester / caller | `principal`, `requester`, `requester_id`, `requesterId`, `requester_agent_id`, `requesterAgentId`, `requesterLabel`, `owner` |
| Subject / agent | `subject`, `agent_id`, `agentId`, `head_agent_id`, `headAgentId`, `carried_agent_id`, `carriedAgentId` |
| Session | `session`, `session_id`, `sessionId`, `session_ref`, `sessionRef`, `parent_session_ref`, `parentSessionRef`, `bind_session`, `bindSession` |
| Route / scope / operator authority | `scope`, `scope_ref`, `scopeRef`, `scope_epoch`, `scopeEpoch`, `route`, `bind`, `identity`, `harness`, `operator_surface`, `operatorSurface` |
| Device / answering machine | `device_id`, `deviceId`, `device_fingerprint`, `deviceFingerprint`, `machine_id`, `machineId`, `console_machine_id`, `consoleMachineId`, `at_console`, `atConsole` |

This is not a closed allowlist of ignored keys: any other key not listed as an
operation param below is also ignored. Do not send extra identity keys. The
identity-parameter test exercises every spelling in the table and proves that
none can turn a non-phone Callosum route into an operator route.

On a non-phone route, list and answer refuse with
`elicitation_operator_required`. Get instead selects the requester interface:
Callosum without a phone bind is refused with `elicitation_requester_refused`.
An allowed card requester uses `elicitation_id` and can read only its own cards;
it may receive `elicitation_not_owner`. That is a different interface, not a
snake_case alias of phone get.

## Shared card shape

Get returns a **full card**. List records and `answer.elicitation` use the same
phone shape, except list omits the two large package bodies noted below. No
response wraps the request in a `request` field. Top-level keys are camelCase;
nested payload keys are not recursively renamed.

| Field | Type and meaning |
|---|---|
| `elicitationId` | String; opaque card id. |
| `kind` | `package`, `flow_install`, `flow_decision`, or `consent`. |
| `title`, `prompt` | Strings to display. |
| `options` | Array of `{id: string, label: string, effect: string, detail?: string}`. Send the exact `id` when answering. Package and installation cards have one grant and one decline. Decision cards have one decline and otherwise `choose` effects. Consent effects are strings, not a closed enum enforced by this interface. |
| `default` | String option id whose effect is `decline`. Do not auto-submit it. |
| `urgency` | `low`, `normal`, or `high`. |
| `onExpiry` | `deny`. Silence never grants. |
| `materialDamage` | Boolean supplied by the requester. |
| `lateExecution` | `execute` or `notify_only`; requester execution policy, not evidence that execution happened. `execute`: a late grant still takes effect (the held action runs, or the next attempt uses the grant). `notify_only`: a late answer has no effect. |
| `target` | `{kind: string, label: string}`. |
| `facts` | Array of `{label: string, value: string}`; present even when empty. |
| `expiresInMs` | Optional integer requested lifetime, 1 through 86,400,000 ms. Omitted on no-deadline cards. |
| `createdAtMs`, `expiresAtMs` | Integer Unix milliseconds. Creation is always present; expiry is omitted on no-deadline cards. Dedup refresh may move or remove expiry without changing id or creation time. |
| `subject` | `{label: string, agentId?: string, headAgentId?: string}`. Missing agent fields are omitted, not null. |
| `requesterLabel` | String requester label, e.g. `reserved:cerebellum`. Display data, not client-supplied identity. |
| `requesterName` | Optional human-readable name for a requester principal Cingulate recognizes, such as `Cerebellum` or `Basal`. Omitted for `direct` and unknown principals; never use the raw principal as a fallback. `requesterLabel` remains the raw principal. |
| `state` | `pending`, `answered`, `expired`, or `withdrawn`. |
| `answeredAtMs` | Optional integer settlement time; omitted while pending. Expiry uses the card's deadline, not the later sweep time. |
| `answeredChoiceId` | Optional string; present after an answer. Expired and withdrawn cards have no selected choice, even though expiry denies. |
| `execution` | Optional string requester-reported execution outcome: `executed`, `approved_not_executed`, or `outcome_unknown`. Omitted until reported. |
| `executionReason` | Optional open-set lowercase snake_case code reported with `approved_not_executed`; absent when no reason was supplied. See the requester execution-report contract. |
| `blockUntilMs` | Optional integer, only on pending cards with an active requester wait. The latest wait deadline, capped at card expiry only if one exists; indicates how long a requester is currently waiting, not an approval, a lease or an extension of expiry. Disappears when the last wait is dropped. Conditional polling accounts for it. |

The feature-gated `secret_use` and `manifest_binding` kinds are specified in
[phone v2](consent-phone-v2.md); a v1 reader must not render them as ordinary
consent cards.

Optional requester-provided fields are omitted when absent: `dedupKey` (string),
`scope` and `duration` (JSON values), `argsDigest` (string), `sessionRef` and
`parentSessionRef` (strings), `preview` (`{label: string, text: string}`), and
`atConsole` (nonempty string). Session references here are card display data,
not caller identity. The request's private `binding` and transient
`requestedSchema` are never served to the phone.

Kind payloads:

* `package`: `{package: string, version: integer, code_hash: string,
  script: string, manifest_json: string}`. `code_hash` is lowercase 64-hex BLAKE3;
  `manifest_json` is JSON encoded **inside a string**. List omits `script` and
  `manifest_json` completely, not as null or empty strings. Get and answer return
  both bodies in full.
* `flowInstall`: a requester-supplied JSON object on `flow_install` cards.
* `flowDecision`: a requester-supplied JSON object on `flow_decision` cards.
* `consent` needs no kind-specific body; browser cards can carry facts and preview.

Flow objects are opaque to the module; do not assume an enforced manifest schema.

## `consent.list_pending`

| Param | Type | Required? | Phone spelling / behavior |
|---|---|---|---|
| `if_revision` | String | No | Send exactly this spelling, copied from a completed first-page poll. Missing, unknown, null or non-string values return a full page. `ifRevision` is ignored. |
| `continuation` | Object | No | Copy the returned JSON object unchanged, including unknown members. Missing or null means first page. Non-null must contain `created_at_ms` (nonnegative signed-64-bit integer) and `elicitation_id` (string). No camelCase aliases. Extra members are ignored. |

There is no `limit`, `since`, agent filter, or caller-selected page size; such
extra params are ignored. Omitted `params` is also accepted for this operation.

Full reply: `{records: Card[], revision: string, continuation?:
{created_at_ms: integer, elicitation_id: string}}`. An empty list is `records: []`.
`unchanged` is absent on full replies. All pending cards are sorted oldest first
by creation time, then lexicographic id (not numerical id suffix), regardless of
whether they have a deadline. Only pending cards appear.
Expired cards are swept before comparing a revision or constructing a page.

For one release, the old `{expires_at_ms, elicitation_id}` object is also accepted
on input: its expiry must be a nonnegative signed-64-bit integer, and Cingulate
resolves the retained card's creation position. If that legacy card has already
been pruned, restart the list. All newly served continuations use creation time
and are JSON objects, never strings, numbers or arrays. An old cursor resumes in
the new creation order, not a frozen copy of the old expiry order.

Each complete serialized response is below 3 MiB. When more cards remain,
`continuation` names the last returned position; send it unchanged to advance
strictly past that position. It is omitted on the final page. Traversal is not a
frozen snapshot: settled cards disappear, and new cards sorting before the
position require a new first-page poll. Get retrieves omitted package bodies.

### Conditional polling

Matching `if_revision` on a first-page request returns exactly
`{unchanged: true, revision: string}` with **no records or continuation**. Retain
the current cards; never interpret this as an empty list. With a non-null valid
`continuation`, `if_revision` is ignored and a full page is always returned.

The revision is opaque. Do not parse it, increment it, compare its ordering or
reuse it across different stores. It covers durable pending-card changes
(creation, answer, expiry, withdrawal, replacement and refresh by dedup), plus
active waiter deadlines used for `blockUntilMs`. The durable portion changes
atomically with those writes and survives reopening the store. A restart that
clears active waits or sweeps downtime expiry cannot incorrectly say unchanged.
It may change even when a particular page renders identically.

Save the first-page revision for conditional polling **only after** collecting
all pages. If revisions differ between pages, restart from the first page to
obtain a complete current list; a continuation is not a snapshot token. If a
poll or page request fails, keep the last completed list and retry, never grant.

Refusals (in addition to shared transport/storage failures below):

| Code | When |
|---|---|
| `elicitation_operator_required` | Route is not a phone operator route. |
| `elicitation_invalid_request` | Non-null continuation is missing its fields or has wrong types/range; or a single projected card cannot fit the page budget. |

## `consent.get`

| Param | Type | Required? | Phone spelling / behavior |
|---|---|---|---|
| `elicitationId` | String | Yes | Send exactly this spelling. `elicitation_id` does not satisfy it; if both exist, only camelCase is read. Empty strings are accepted syntactically and normally not found. |

Success result is the full `Card`, not `{elicitation: Card}`. An answered card
includes its selected `answeredChoiceId` and original options and labels. An
expired or withdrawn retained card returns that full card with its terminal
`state` and `answeredAtMs`; settlement does not make it disappear. Unknown ids
return the typed refusal `elicitation_not_found`, **never null**.

Phone `consent.get` keeps settled cards readable whether or not the requester has
acknowledged them. It returns the card until the existing 24-hour retention period
ends and pruning removes its payload. The phone cannot distinguish a pruned card
from an unknown id. Requester-side get and answer-feed visibility still follow the
acknowledgment rules in the requester contract.

| Code | When |
|---|---|
| `elicitation_invalid_request` | Missing or non-string `elicitationId` on a phone route. |
| `elicitation_not_found` | Unknown, pruned or no-longer-retained id. |
| `elicitation_requester_refused` | Non-phone route whose principal cannot use the requester interface (including non-phone Callosum). |
| `elicitation_not_owner` | Only on the separate requester get interface: id does not belong to its route owner. Not a phone-route refusal. |

## `consent.answer`

| Param | Type | Required? | Phone spelling / behavior |
|---|---|---|---|
| `elicitationId` | String | Yes | No `elicitation_id` alias. |
| `choiceId` | String | Yes | Exact option id, not label/effect. No `choice_id` alias. |

The first settlement wins. Terminal-state and retention checks happen before
choice validation and the answering-machine check.

| Card / attempted answer | Normal result |
|---|---|
| Pending, valid choice, machine check passes | `{ok: true, alreadyAnswered: false, elicitation: Card}`; card is now `answered` with that choice. |
| Already answered, same choice | `{ok: true, alreadyAnswered: true, elicitation: Card}`; no new answer is committed. |
| Already answered, different choice (even an unoffered id) | `{ok: false, code: "conflict", elicitation: Card}`; returned card holds the committed choice. |
| Expired | `{ok: false, code: "conflict", elicitation: Card}` with `state: "expired"`, no selected choice. This also applies at the exact deadline. |
| Withdrawn | `{ok: false, code: "conflict", elicitation: Card}` with `state: "withdrawn"`, no selected choice. |
| Unknown or outside settlement retention | `{ok: false, code: "not_found"}`; no `elicitation`, no `alreadyAnswered`. |

Acknowledgment does not hide a card from phone get or from answer's retained-record
check. A settled card still returns `conflict` from answer after acknowledgment,
until retention ends. Never infer a grant from get's not-found refusal.

| Refusal code | When |
|---|---|
| `elicitation_operator_required` | Route is not a phone operator route. |
| `elicitation_invalid_request` | Required id/choice missing or not a string. |
| `elicitation_invalid_choice` | A still-pending card does not offer `choiceId`. Card remains pending; no answer took effect. |
| `user_not_at_console` | A still-pending card requires `atConsole` and the route has no matching attested answering machine. Body `machineId` / `atConsole` cannot bypass this. Currently phone route binds have no attested console machine, so these cards fail closed. |

### Lost or ambiguous answers

A normal answer result reports the committed state unambiguously: a conflict
means the attempted answer did not replace it. The phone can become uncertain
when the request is cancelled, the transport disconnects/times out, or a storage
error prevents an authoritative response. Cancellation can happen after the
answer transaction committed, while post-commit notification is still awaited;
notification failure itself does not undo the answer or fail the operation.

Resolve uncertainty with `consent.get(elicitationId)` on the phone route. If
`answered`, display its `answeredChoiceId` (not the locally attempted choice).
If pending, no settled answer is recorded at that read; retrying the same choice
is safe with respect to first-settlement-wins. If expired/withdrawn, display that
terminal state and do not grant. If get refuses not-found or cannot reach storage,
the outcome cannot be recovered from these operations; show it as unavailable,
not approved. A repeated same-choice answer may help recover a retained answered
card even after acknowledgment. No timeout, silence or error is approval.

## Shared refusal codes and precedence

The following shared refusal codes can occur when calling `consent.list_pending`,
`consent.get`, or `consent.answer`:

| Code | When |
|---|---|
| `elicitation_request_too_large` | Complete request envelope exceeds 1,310,720 bytes; checked before JSON decoding. |
| `invalid_request` | Invalid JSON envelope, missing/non-string `method`, or otherwise undecodable envelope. |
| `unknown_method` | Method is not supported (including misspelling an operation). |
| `consent_unavailable` | Store is not open/ready, including during failed startup or lease retry. |
| `elicitation_store_error` | Storage, SQLite or persisted-JSON failure during sweep, read or transaction. Do not treat it as approval. |
| `cancelled` | Production transport cancels the request; the handler may already have committed an answer. |

After envelope decoding and method recognition, availability is checked before
route admission. Admission is checked before required ids/choices. Valid ids and
choices are parsed before sweeping; list continuation is validated after the
sweep. A read or answer refusal may therefore still have expired unrelated due
cards; it never grants them. Transport failures outside the handler may have
their own Callosum codes, which this contract does not invent.

## Executable checks

`crates/cingulate-vectors/tests/phone.rs` checks the served examples, hashes,
param spellings, identity-key behavior, answer outcomes and get retention.
`crates/ck-cingulate/tests/module.rs` checks conditional polling, all durable
pending events, read-triggered expiry, reopen, continuation and waiter deadlines.
Existing module and store tests also check console refusal, authorization,
transaction rollback, bounded pages and cancellation. These are checks of the
implemented interface, not promises of unimplemented card features.
