# Consent card text contract, v1

Cingulate can carry two optional, human-readable fields on `flow_install` and
`package` approval cards. It does not interpret or synthesize their wording; the
producer owns it. Producers send these fields only when Cingulate advertises the
`consent-card-text/v1` capability in its manifest `provides` list.

The separate [author explanations extension](consent-card-explanations-v1.md)
adds `explanation` and `changes` under `consent-card-explanations/v1`; support for
`consent-card-text/v1` alone does not imply support for those members.

## Request fields

Both fields use snake_case in `consent.request` params. They are accepted only
when `kind` is `flow_install` or `package`; any other kind carrying either field
is refused with `elicitation_invalid_request`, and the refusal message names the
field. The nested objects are closed, all fields have the stated string or array
types, and null is not an alternate spelling for absence. Unknown nested keys,
wrong types, and values outside these bounds are refused with
`elicitation_invalid_request`; messages identify the affected field and any
applicable limit. Cingulate never truncates input. The existing 1,310,720-byte
limit still applies to the complete framed request.

| Request field | Shape and limits |
|---|---|
| `source` | `{language: string, text: string}`. Both values are nonblank; `language` is at most 32 UTF-8 bytes. `text` has no separate size cap beyond the whole-request limit. |
| `dry_run_text` | `{summary: string, would: [string], problems: [string]}`. `summary` is nonblank and at most 300 UTF-8 bytes. Each array has at most 8 items; empty arrays mean nothing would happen or no problems were found. Every item is nonblank and at most 200 UTF-8 bytes. |

Every string in `dry_run_text` must be free of ASCII control characters
(U+0000–U+001F and U+007F). A control character is a producer defect and is
refused as `elicitation_invalid_request`, naming `dry_run_text`. Cingulate does
not parse, execute or otherwise interpret either field.

## Storage and readbacks

The values are stored verbatim with the card. They participate in pending-card
deduplication: changing either value replaces the pending card rather than
refreshing it as unchanged. Existing JSON records omit these fields and remain
readable without a store migration.

Requester readbacks preserve the snake_case names `source` and `dry_run_text`.
Phone projections expose `source` and `dryRunText` on `consent.list_pending`,
`consent.get` and `consent.answer` readbacks. For both audiences, absent fields
are omitted. Package list records continue to omit only the large `script` and
`manifest_json` bodies; these human-readable fields remain available.

The served-record examples are in
[`test-vectors/consent-card-text-v1/`](../../test-vectors/consent-card-text-v1/README.md).
Their generator captures actual Cingulate handler replies.

## Federated caller names

For federated `consent` cards, the provider sends an action phrase such as
`read your mail.`. Cingulate supplies the computer-first sentence from the
[daemon-stamped subject](consent-requester-v2.md#federated-consent-carriers),
never from a requester's body or a local agent resolver. Since ck-cingulate
0.16.1, optional `remote_claim.agent_name` (agent, run or worker) and
`remote_claim.flow_name` (flow) are display text. A worker's `agent_name` names
only its `on_behalf_of` agent; without that id it is not rendered.

For paired computer `Juniper`, name `Remote Sage` or `Daily digest`, and action
`read your mail.`, the exact sentences are:

| Claim | With a name | Without a name (unchanged) |
|---|---|---|
| agent `Sage` | `“Juniper” says agent “Remote Sage” (Sage) wants to read your mail.` | `“Juniper” says agent Sage wants to read your mail.` |
| worker for `Sage` | `“Juniper” says a worker for agent “Remote Sage” (Sage) wants to read your mail.` | `“Juniper” says a worker for agent Sage wants to read your mail.` |
| flow `Morning-digest`, version 3 | `“Juniper” says flow “Daily digest” (Morning-digest) (version 3) wants to read your mail.` | `“Juniper” says flow “Morning-digest” (version 3) wants to read your mail.` |
| run for `Sage` | `“Juniper” says a run for agent “Remote Sage” (Sage) wants to read your mail. (this time only)` | `“Juniper” says a run for agent Sage wants to read your mail. (this time only)` |

Names contain 1–80 Unicode scalar values of plain text, with no Unicode control
characters or bidi formatting characters (U+061C, U+200E–U+200F, U+202A–U+202E,
U+2066–U+2069). Cingulate rechecks these bounds even on typed daemon stamps.
An invalid name is dropped, not truncated and not a refusal; the unnamed
sentence is used. Worker-without-attribution and claimless sentences are
unchanged. No name is resolved from the id.

Validated names are stored in `subject.federated.remote_claim` with their
protocol snake_case spelling, unchanged on both phone and requester readbacks.
The same optional fields appear in `consent.watch_pending` changed items under
`card.subject.federated.remote_claim`; no watch envelope fields are added.
`subject.label` uses the [permissions label](permissions-v1.md#federated-grants).
Names never enter ownership or standing-grant keys: renaming or removing a name
does not affect collection of an existing card, including its one-time answer.

## Sandbox-escape card text (0.28.0)

Unlike producer-authored card text, a `sandbox_escape` card uses fixed wording.
The title is “Run one command outside the sandbox?” The prompt is “Runs outside
the `<Level>` sandbox, with full access to your logins and files.” Substitute
`Credentials`, `Standard` or `Strict` for `<Level>` according to `escapes_level`.

Show the stamped agent's name and, when `subject.viaWorker` is true, “A worker
asked.” Display the exact command as plain data in a fixed-width block. Preserve
every character, including tabs, line feeds and surrounding whitespace; allow
scrolling rather than ellipsizing, summarizing, or otherwise hiding any text.
Show the folder and shell labelled “Folder” and “Shell”. If `background` is true,
show “Keeps running after you answer.” Show `reason` separately as plain text
labelled “Agent's words”, never as card instructions. Show when the card expires.

The options are “Run once” and “Don't run”, with “Don't run” the default. See
the [phone contract](consent-phone-v2.md#sandbox-escape-approvals-0280) for which
surfaces may render and answer the card.
