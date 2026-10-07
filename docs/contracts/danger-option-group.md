# Danger option group on consent cards

Status: contract for `consent/v1`, not yet implemented. Vectors:
[`test-vectors/danger-option-group-v1/`](../../test-vectors/danger-option-group-v1/README.md).
Consumers: Cerebellum (files cards with danger options), the phone app and other
answering surfaces (declare the feature), cingulate (enforces it).

The key words MUST, MUST NOT and MAY are normative.

## Why

Some grants are much broader than the request that prompted them: "Use the whole
computer" includes taking over the mouse and keyboard. A surface that does not
know to mark such an option would show it as one more button. The danger group
lets a requester offer it while guaranteeing it reaches only surfaces that
present it as dangerous.

## The option field

A card option (`consent.request` `options[]`, shape `{id, label, effect,
detail?}`) MAY carry `group`. The only defined value is `"danger"`.

```json
{"id": "computer_30m", "label": "Use the whole computer for 30 minutes",
 "effect": "grant", "group": "danger",
 "detail": "Includes taking over your mouse and keyboard."}
```

Example, Cerebellum's app card ([`file-card`](../../test-vectors/danger-option-group-v1/file-card.json)):

| id | label | effect | group |
|---|---|---|---|
| `app_30m` | Use TextEdit for 30 minutes | grant | |
| `app_always` | Always allow TextEdit | grant | |
| `deny` (default) | Don't allow | decline | |
| `computer_30m` | Use the whole computer for 30 minutes | grant | danger |
| `computer_always` | Always allow the whole computer | grant | danger |

The current decoder sets no upper bound on option count for kind `consent`, so a
five-option card is valid; a surface without the feature sees three.

## Filing rules

`consent.request` refuses, writing nothing:

1. **Unknown group.** `group` with any value other than `"danger"`:
   `elicitation_invalid_request`. This is part of the option's shape check, so it
   precedes every semantic rule, like any other malformed field
   ([`refuse-unknown-group`](../../test-vectors/danger-option-group-v1/refuse-unknown-group.json)).
2. **Danger default.** The default option (the one applied on silence) is in the
   danger group: `elicitation_fail_closed_required`. Checked immediately after the
   existing rule that the default is a decline and `on_expiry` is `deny`, which is
   unchanged ([`refuse-danger-default`](../../test-vectors/danger-option-group-v1/refuse-danger-default.json)).
3. **Only danger grants.** The card has options with effect `grant` and all of
   them are in the danger group: `elicitation_invalid_request`. A card with no
   grant options is unaffected
   ([`refuse-only-danger-grants`](../../test-vectors/danger-option-group-v1/refuse-only-danger-grants.json)).

Rule 2 guarantees stripping never removes the default; rule 3 guarantees a card
still offers a non-dangerous grant after stripping. `group` takes part in dedup
comparison like every other request field, and a `requestedSchema` decision enum
still lists every option id, danger options included.

A cingulate without this feature refuses `group` as an unknown field
(`elicitation_invalid_request`, ``unknown field `group` ``). A requester that gets
that refusal MAY refile without its danger options. It MUST NOT move a danger
option into the default or relabel it without the group.

## Declaring the feature

A surface declares support per call with `clientFeatures`, an array of strings,
on every surface operation that returns card options:

- `consent.list_pending`
- `consent.get` (surface projection)
- `consent.answer` (its reply carries the card)

Until the cutover the phone reaches these through core's legacy relay as
`elicitation.list_pending`, `elicitation.get` and `elicitation.answer`; the same
rule applies there. Feature names the receiver does not know are ignored.
`clientFeatures` that is not an array of strings is `elicitation_invalid_request`.

```json
{"elicitationId": "el_1", "clientFeatures": ["option-group"]}
```

## Stripping

If the call does not declare `option-group`, cingulate removes every option with
`group: "danger"` from `options` in the reply. Nothing else changes: `default`,
`state`, `answeredChoiceId` and every other field are as for a declaring caller.
A settled card can therefore carry an `answeredChoiceId` that is not among the
options shown; surfaces MUST tolerate that. The card stays answerable through its
remaining options.

Vectors: [`list-pending-declared`](../../test-vectors/danger-option-group-v1/list-pending-declared.json)
and [`list-pending-undeclared`](../../test-vectors/danger-option-group-v1/list-pending-undeclared.json);
[`get-declared`](../../test-vectors/danger-option-group-v1/get-declared.json)
and [`get-undeclared`](../../test-vectors/danger-option-group-v1/get-undeclared.json).
The undeclared replies equal today's projection of the same card filed without
its danger options (`crates/cingulate-vectors/tests/contracts.rs`).

Requester reads (`consent.get` on the owner route, `consent.await`,
`consent.withdraw`, `consent.report_execution`, `consent.answers`) return the
card as filed. The requester wrote the `group` values, so there is nothing to
protect it from. Push pointers carry no card content and are unaffected.

## Answering

`consent.answer` whose `choiceId` names a danger option, from a call that did not
declare `option-group`, refuses with `elicitation_option_group_required`. The
message names the option. Nothing is written, settled or notified. The check runs
once the card is found, before the state, choice and `at_console` checks, so it
answers the same for pending and settled cards
([`answer-danger-undeclared`](../../test-vectors/danger-option-group-v1/answer-danger-undeclared.json)).
With the feature declared the answer proceeds as today
([`answer-danger-declared`](../../test-vectors/danger-option-group-v1/answer-danger-declared.json)).

A surface that has not shown an option cannot honestly answer with it, so this
refusal is never retried by adding the feature to the same call: the surface
re-reads the card with the feature declared and shows it first.

## Requirements on surfaces

A surface that declares `option-group` MUST present danger options as distinct
from the others (for example separated and visually marked) and MUST NOT make
one the preselected or primary action.
