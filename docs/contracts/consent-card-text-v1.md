# Consent card text contract, v1

Cingulate can carry two optional, human-readable fields on `flow_install` and
`package` approval cards. It does not interpret or synthesize their wording; the
producer owns it. Producers send these fields only when Cingulate advertises the
`consent-card-text/v1` capability in its manifest `provides` list.

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
