# Tool-permission asks

Status: **proposed, not approved for building.**

When AFT's policy says a tool call needs permission ("ask"), something has to put that question
to the right party and bring the answer back. Ufuk ruled on 2026-10-10 that this is Cingulate's
job, not prefrontal-core's, so that every approval in the ecosystem goes through the one consent
module, with one card format, one answer rule and one audit trail. This design replaces
prefrontal's `docs/designs/access-asks-core.md`, a draft that had core doing the routing, and
reuses its decisions where they still fit; "keeps D1" and the like name the decision of that
draft a section carries over.

**Nothing here is built.** Today Cingulate accepts neither kind below from AFT, and answers only
come from the phone. Every rule below about requesters, answerers and legs is a change to make.

## Who does what

| Job | Owner |
|---|---|
| Decide allow, ask or deny for a tool call or command; keep workers inside their worktree | AFT |
| Hold the call while an ask is open; store grants keyed by scope and path pattern; return a named deny | AFT |
| Keep the ask record, choose who answers, show it on every surface that can answer, take the first answer, withdraw the rest, expire, audit | Cingulate |
| Wake a head about a new ask from its worker; relay the head's answer | core |
| Render cards on the phone and desktop app | the apps, from Cingulate's records |

AFT never chooses an answerer. Cingulate never decides policy and never reads AFT's rules.

## Identity

- **Asker:** AFT sends the ask as `reserved:aft`, carried under the asking session's scope. Cingulate
  reads the session, its agent and its harness from the daemon's bind stamp, never from the body.
  Core must list `reserved:aft` as a carrier toward `cingulate` on head and worker scopes (core
  change 1 below).
- **Worker to head:** core already sets `parent` on every worker's scope record, naming its head's
  scope at the head's current epoch; it follows the head across epoch changes and is dropped when
  the head ends. The daemon's bind stamp carries it, so Cingulate takes the head from the stamp
  and never asks core or Entorhinal. Worker and head are told apart by the scope's `kind`, never by
  whether a `parent` is present: a `worker` scope with no `parent` (its head has ended) is refused
  `access_no_head`, so an orphaned worker's ask never reaches the user directly.
- **Codemode runs:** a run's calls are judged as calls of the agent that started the run. A run
  scope is `kind: ephemeral` with `agent_id` and `run_id`. Only heads can start runs today, so a
  run scope has no `parent` and its ask is that head's own ask. If workers are later allowed to
  start runs, core will set the run scope's `parent` to the worker's scope, and Cingulate will
  walk run, then worker (through the daemon's `scope.describe`), then head; nothing else changes.
  AFT ends a run's hold early when the run has less time left than the leg, and withdraws the ask.
- **Which calls ask through Cingulate (changes D2's test):** a call whose stamped scope is owned by
  `reserved:prefrontal-core` with `kind` `head`, `worker` or `ephemeral`. Testing `agent_id` alone
  would miss every worker, because worker scopes carry no `agent_id`. A call with no stamp, or a
  scope from any other owner, belongs to AFT alone, and its plugin asks in the host UI as it does
  without Alfonso. If Cingulate is not running,
  AFT denies an Alfonso agent's call with `access_unavailable`; it never falls back to a host
  prompt for a worker, since nobody would answer it.

## Routing (keeps D1)

- **Worker ask:** the answerer is the worker's head. The user sees it only if the head escalates.
- **Head ask:** the answerer is the user, on every surface that can answer now: the head's host
  UI (OpenCode or Pi, when AFT's plugin reports a UI attached) and a phone card. With a host UI,
  the card is raised only if the host has not answered within 3 s, so a call the harness allows
  by its own rules never flashes a card on the phone. A Broca (app-driven) head gets the card at
  once.
- **Escalated worker ask:** goes to the head's user surfaces, as a head ask does.

## Kinds

- **`tool_permission`**, requested by `reserved:aft` only. Body: `call_key`, `tool`,
  `operation` (`read`, `write` or `exec`), `paths` (at most 16 absolute paths), `command`
  (exec only, at most 600 characters), `reason` (one line, at most 160 characters), `args_hash`,
  optional `schema_pin`, `pattern` (what AFT would store for a scope grant) and optional
  `head_policy` (AFT's own verdict for the same operation under the head's scope). Options are
  fixed: `allow_once` and `allow_scope` (effect grant) and `deny` (effect decline); `deny` is the
  default and the expiry outcome. `allow_scope` is left out when the answerer may not grant it.
- **`macos_permission`** (keeps D9), requested by `reserved:aft` only, informational: no options,
  nothing waits on it. It names the folder, the program to grant and the steps in System
  Settings. One open card per (folder, program): a repeat request returns the open card. AFT
  closes it with `consent.withdraw` when it sees the folder readable again. Refused on platforms
  other than macOS.

## Answers (keeps D5)

| Answer | Effect for AFT | Who may give it |
|---|---|---|
| `allow_once` | grant for this `call_key`/`args_hash` only | the worker's head; the user |
| `allow_scope` | grant for the asking scope until that scope epoch ends | the head, for its worker, only when `head_policy` is `allow`; the user |
| `deny` | decline, with an optional one-line note to the asker | the head; the user |
| `escalate` | moves the ask to the user | the head only |

A grant always applies to the asking session's scope. A user's answer on an escalated ask applies
to the worker's scope, not the head's.

## How each party answers

- **Head (changes D4's carrier, keeps its surface):** the head answers with the work tool it
  already has, `work(action="prompt", id="acc_…", prompt="<answer>[: note]")`, where `acc_…` is
  the ask's id from the notice. The work tool already routes prompts by id prefix, so no head's
  tool list or cached prompt changes. Core relays it to Cingulate as `consent.answer` from
  `reserved:prefrontal-core` with body `{elicitation_id, answer, note?, answered_by: {agent_id,
  scope_ref, scope_epoch}}`. Core fills `answered_by` from the calling head's own verified session,
  never from the prompt text, and refuses locally when that head is not the asking worker's
  parent, so a wrong head gets a clear error at once. Cingulate accepts the relay only on a
  `tool_permission` card still in its head leg, only when `answered_by` matches the `parent` in
  the asking worker's stamp, and only with the four answers above. Core is already the authority that
  assigns agent identity, so relaying a head's answer gives it nothing new.
- **User on the phone:** the existing phone rule (`consent.answer` from `reserved:callosum` with a
  harness starting `fed-phone:`), plus a new restriction: only while the ask is in its user leg.
- **User in the host UI:** AFT holds a `consent.host_feed` long-poll to Cingulate for each head
  session whose plugin is live, and relays `present` and `withdraw` entries to its plugin. The
  plugin's result returns as `consent.answer` from `reserved:aft` with `via: host_ui`, accepted only
  for a card presented to that session's host. The feed subscription names the operations the
  host can render (`ops`, from `read`, `write` and `exec`): today OpenCode renders all three and Pi
  none. Cingulate presents a host prompt only for a listed operation; any other ask goes to the
  card at once, with no 3 s grace. AFT's plugin never shows a prompt on its own for
  an Alfonso agent's call: it shows only what the feed presents.

The first answer to reach Cingulate's record wins (a compare-and-set in one store transaction).
Cingulate then withdraws every other surface. A late answer is refused with the settled answer, and
a host prompt that cannot be withdrawn shows "Already answered".

## AFT's side of the record

- **Waiting:** `consent.await {elicitationId}` is a long-poll that returns when the card settles
  or at the caller's wait bound. It reads only, so AFT may re-await at any time.
- **Withdrawing:** when a held call ends without an answer (client cancelled, route closed, a run's
  scope ended, or the asker gone), AFT calls `consent.withdraw {elicitationId}`. Only the principal
  that raised a card may withdraw it. The record moves to `access_withdrawn`.
- **After an AFT restart:** AFT re-sends `consent.request` with the persisted `call_key`; the same
  `call_key` returns the same ask, and AFT awaits or withdraws it.

## Expiry and waking (keeps D6)

- **Head leg: 90 s.** AFT's hold must end under Broca's 120 s tool-call ceiling. On expiry:
  `access_head_no_answer`.
- **User leg: 10 min** (head ask, or escalated worker ask from the escalation). AFT ends a held
  call at its route's reply budget (at most 100 s) with `access_pending`, not a deny; when the
  ask settles, the asker is woken with the result and retries, and AFT matches `allow_once` to the
  same `args_hash`. On expiry: `access_user_no_answer`.
- **Waking a head for its worker's ask:** Cingulate calls core's `session.notice` (core change 2
  below). No phone push goes out for the head leg.
- **Waking a parked asker after settlement:** the same `session.notice`, with a different purpose.
- **Busy heads:** a head in the middle of a long tool call sees the notice only at its next step
  (Pi steers between calls, OpenCode at the next step), so some head legs will expire on busy
  heads. `access_head_no_answer` is the intended outcome there, and the audit will show it.

## What core builds

1. **Carrier listing:** `reserved:aft` listed as a carrier toward `cingulate` on head and worker
   scope records, unless today's carrier list already covers it.
2. **`session.notice`**, admitted only from `reserved:cingulate`, with no answer path, refusing
   sessions core does not hold. Body: `{session_ref, elicitation_id, purpose, fields}`, where
   `purpose` is one of `head_ask`, `escalated`, `settled` or `withdrawn`, and `fields` are
   structured (`tool`, `operation`, `paths`, `command`, `reason`, `head_policy`, `answers`,
   `expires_at_ms`, and for `settled` the outcome and note). Core deduplicates on
   `(elicitation_id, purpose)`, so a retry never wakes twice while each step of one ask still
   wakes once. Core renders the text itself and escapes every field, so a worker's `reason`
   cannot inject instructions into its head's session, and every notice reads like core's
   others.
3. **The `acc_` arm of `work(prompt)`**, accepting only the four answers and relaying them as
   described above.

## Refusals (keeps D8)

Every refusal has a name, and none grants: `access_no_head` (no parent head, or its scope not
live), `access_head_gone`, `access_not_placed` (no surface can answer now),
`access_head_no_answer`, `access_user_no_answer`, `access_withdrawn` (the call was cancelled),
`access_declined` (with the note), `access_unavailable` (Cingulate unreachable, returned by AFT).
A head answering an ask that is not its own worker's, or `allow_scope` outside its authority, is
refused to the relay and the ask stays pending.

## State machine

One record per ask, in Cingulate's store:

```
request ─┬─ worker ask ─▶ [pending:head] ─escalate─▶ [pending:user] ─first answer─▶ [settled]
         │                      │ answer ─────────────────────────────────────────▶ [settled]
         │                      └─ 90 s ─▶ [refused: access_head_no_answer]
         └─ head ask ──────────────────────▶ [pending:user] ─ 10 min ─▶ [refused: access_user_no_answer]
any pending state ─ head gone / call withdrawn ─▶ [refused: access_head_gone | access_withdrawn]
```

A second request with the same `call_key` returns the same ask; an asker never re-asks.

## Audit

Every settled record keeps the asker, its head, the scope ref and epoch, the request fields,
the surfaces offered, every answer attempt with surface and time (losers included), any
escalation note, and the final state. Standing grants stay with AFT (`grants.list`,
`grants.revoke`), as for every provider.

## Open questions for Ufuk

1. The 90 s head leg and 10 min user leg.
2. Whether to show a host prompt that cannot be withdrawn beside the phone card, or the card only.
3. Whether a user's "allow for this task" on an escalated ask should also be offered for the
   head's scope.
4. Whether a head may `allow_scope` only when `head_policy` is `allow`, or also when it holds a
   covering grant.
