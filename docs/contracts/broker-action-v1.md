# Broker-action approval, v1

Cingulate 0.27.0 advertises `broker-action-approval/v1`. An approving reader
advertises `clientFeatures: ["broker-action-approval"]` once it can decode,
render and answer this kind. Support for another kind is not sufficient.
Deploy readers before enabling filers.

## Filing

Use `consent.request` with exactly this body:

```json
{"kind":"broker_action","broker_action":{"statement_b64url":"<vault statement, unchanged>"}}
```

Only `reserved:aft` and `reserved:plexus` may file, on daemon-stamped routes
whose scope stamp names the statement's `agent_id`. Any other requester is
refused with `elicitation_requester_refused`. The body cannot supply titles,
prompts, labels, options or an expiry override. Cingulate supplies the card
metadata and defaults to decline. The card expires at the statement's
`expires_at_ms`. Filing at or after that time refuses `elicitation_expired`.
Identical statement bytes filed by the same owner return the existing card;
a new request needs a new statement and card. Withdraw with `consent.withdraw`.

## Statement

The statement is strict unpadded base64url of RFC 8785 canonical JSON, at most
512 KiB decoded. Its keys are closed:

| Member | Meaning |
| --- | --- |
| `v` | `ck-broker-action-statement/v1`. |
| `request_id` | A ULID, never reused. |
| `agent_id`, `scope` | The minting route's agent and scope. The scope has exactly `owner`, `kind`, `ref`, and `epoch`. |
| `service`, `identity` | `github` in v1 and `github_app:<slug>`. |
| `action` | The enforcer's action name, equal to `kind` for one leg. |
| `host`, `resource` | The request host and the binding resource (`owner/repo`). |
| `policy_version`, `policy_sha256` | The active policy's integer version and its lowercase hexadecimal SHA-256. |
| `fields` | Up to 16 entries in consent-template order, each exactly `{name, value}`. Values are JSON; strings are verbatim. Numbers are integers in [−(2^53−1), 2^53−1], never floats. |
| `kind`, `method`, `request_sha256` | The policy kind, HTTP method, and lowercase hexadecimal request SHA-256. Required for one leg; absent when `legs` is present. |
| `legs` | Optional array of 1–16 `{kind, method, resource, request_sha256}` entries in send order. A single entry is valid when optional legs were skipped. |
| `context` | Optional display-only masked command or action, at most 200 characters. No Unicode control (Cc), format (Cf), line-separator (Zl) or paragraph-separator (Zp) characters. |
| `expires_at_ms` | The card deadline, in Unix milliseconds. |

The request digest hashes `ck-broker-request/v1` followed by a zero byte and
four parts, each a 32-bit big-endian byte length followed by its bytes. The
parts are the uppercase ASCII method, lowercase ASCII host (with `:port` only
for a non-default port), exact request target (path and, if present, `?` and
raw query), and exact body (zero length if absent). The enforcer computes the
digest before adding the credential header, which carries the request-scoped
access token the vault issues for this approved request. Use the request as it
will be sent, excluding that header and encoding only the four parts above.
Cingulate checks the statement grammar, not signatures, template membership,
active policy, or the underlying request bytes. A defect refuses
`elicitation_invalid_request` naming `broker_action.<field>`.

## Rendering and answering

In release builds, only an approving phone's daemon-stamped route with the
feature may list, watch, get or answer cards. Unsupported readers cannot see
the cards or their watch removal ids. Unsupported operator get refuses
`elicitation_not_found`; unsupported operator answer refuses
`elicitation_broker_action_approval_required`. Agents and computers are not
answering surfaces. A development build with `dev-local-operator` also
admits the stamped local-operator desktop route with the feature; its
fingerprint is `00000000000000000000000000000000`.

The flat reader card has `kind: "broker_action"`, top-level `expiresAtMs`,
and `brokerAction: {statement_b64url, statement_sha256, request_id, agent_id,
service, identity, action, resource}`. Nested names remain snake_case.
Independently decode the canonical statement and check its digest against the
card's `statement_sha256`. Refuse a card whose facts or expiry disagree with
the statement. Render from those bytes, not filer text. Show
“Allow this action once?”, the agent's display name from its agent record and
its service identity, then the service, action, resource and method. For
`legs`, show a row for each leg in send order. Show the field labels exactly
as named and let the person reveal every byte of any shortened value. Show
`context` last as “Agent's command”, when present. The buttons are “Allow
once” and “Decline”, with decline as the default.

Decline calls `consent.answer` with `{elicitationId, choiceId: "decline",
clientFeatures: ["broker-action-approval"]}` and no envelope. Send nothing to
the vault. For allow, sign the exact canonical envelope and send
`{card_ref, envelope_b64url, signature_b64url}` to the vault's
`policy.approve_action`, using the card id as `card_ref`. After the vault
accepts, call `consent.answer` with `{elicitationId, choiceId: "allow",
clientFeatures: ["broker-action-approval"], envelope_b64url}` using the same
envelope bytes. Do not send a signature to Cingulate.

The envelope is strict unpadded base64url of RFC 8785 canonical JSON, with
exactly `{v, request_id, statement_sha256, decision, device_fingerprint,
issued_at_ms}`. Cingulate checks `v == "ck-broker-action-envelope/v1"`,
`decision == "allow"`, the recorded request id and exact statement digest,
and the fingerprint of the answering route. Invalid bindings refuse
`elicitation_invalid_request` without recording an answer. Cingulate does
not verify signatures or grant tokens.

An accepted answer adds `brokerActionAnswer` and `answeredDevice` to the flat
card. An identical answer retry does not record another answer or replace it
with a different envelope. Answers at or after expiry refuse
`elicitation_expired`. Expired cards remain
readable to approving devices for 24 hours in state `expired`. List them in
“Missed” with agent, action, resource and time, with no answer buttons. Only a
new card can approve the action.

## Requester record and notices

The requester record exposes the same facts under `broker_action`. After an accepted answer
it also has `broker_action_answer: {decision, statement_sha256,
envelope_sha256?}`. Only allow has `envelope_sha256`, the SHA-256 of the
exact decoded envelope. Expiry has state `expired` and no answer object; it
is not a decline. Do not act on the Cingulate answer alone: the vault must
verify and consume the approval for the exact request (or each ordered leg).

Escalate, settle and withdraw notices carry only Cingulate metadata. They
never include the statement, resource or fields. Fetch the card on an
admitted route to read the payload.

See the [handler-generated samples](../../test-vectors/broker-action-v1/README.md).
