# Consent card explanations contract, v1

Since ck-cingulate 0.17.0, the manifest's `provides` list includes
`consent-card-explanations/v1`. Core sends the fields below only when that
capability is declared. Older releases do not declare support.

## `consent.request`

Only `kind: "package"` and `kind: "flow_install"` accept these optional members:

| Member | Shape | Limits |
|---|---|---|
| `explanation` | `{label: string, text: string}` | `label`: 1–64 Unicode scalar values; `text`: 1–8,192 UTF-8 bytes |
| `changes` | `{label: string, text: string}` | `label`: 1–64 Unicode scalar values; `text`: 1–8,192 UTF-8 bytes |

Both members are independent and may be omitted. An explicit `null` is not
absence. Objects are closed: no keys other than `label` and `text` are accepted,
and both strings are required. Neither string may contain control characters
U+0000–U+001F or U+007F–U+009F, except newline U+000A. In particular, tabs and
carriage returns are refused. Newlines are allowed in either string. Limits
count bytes for text, scalars (not graphemes or UTF-16 units) for labels. Empty
strings are refused; whitespace is preserved, not trimmed. The existing
whole-request limit of 1,310,720 bytes also applies.

These are plain author text, displayed with the labels core supplies. The flow
author describes what the flow does in `explanation`, and from flow version 2 onward
describes changes in `changes`. Core constructs the card using those texts and
its own check against the approved version. Cingulate neither synthesizes that
check nor enforces version-dependent presence: both members remain optional.
Apps render the supplied strings as plain text, not HTML or Markdown, and must
not treat author claims as verified facts or instructions.

Invalid shapes, caps or characters return `elicitation_invalid_request`, naming
the affected field, such as `explanation.text`. On any other kind, either member
is refused by name, never silently dropped. Unknown members are still refused.
Every card defaults to a decline and expires with
`on_expiry: "deny"`; silence, a timeout or an error never grants anything.
No identity comes from these objects: requester and subject identity remain
daemon-stamped.

## Storage, `consent.get` and `consent.list_pending`

Both objects are stored verbatim in the card's durable `record_json`; no table
columns or migration are needed. Older stores with neither member still open.
The objects participate in card equality and pending-card deduplication like
other request fields.

`consent.get` returns both objects unchanged on requester and app projections.
Every `consent.list_pending` record carries them unchanged when present, even
for package cards whose script and manifest bodies are omitted. The names
`explanation` and `changes`, and nested `label` and `text`, are the same on
snake_case requester and camelCase app surfaces. An absent member is omitted,
never projected as null. The shared card projection also includes them in
answer readbacks and changed-card watch items.

Neither object, including its label, enters push pointers or the durable
outbox's escalated, settled or withdrawn notices. Those notifications deliberately
omit requester-written prompt text; apps fetch the card to display author text.

## Pinned app samples

[`test-vectors/consent-card-explanations-v1/`](../../test-vectors/consent-card-explanations-v1/README.md)
contains handler-generated `consent.get` and `consent.list_pending` replies for
both card kinds, shared by the phone and desktop apps. The invented Meadow
Digest flow demonstrates both objects, Unicode and multiline plain text.
