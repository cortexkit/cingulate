# Tool-permission asks

Status: **approved for building** (Ufuk's rulings of 2026-10-11 are folded in; see the last
section).

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
| Keep the ask record, choose who answers, show it on every surface that can answer, take the first answer, withdraw the rest, audit | Cingulate |
| Wake a head about a new ask from its worker, remind it until it answers; relay the head's answer | core |
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
  When a run's scope ends (its deadline passes), AFT withdraws the run's open ask.
- **Which calls ask through Cingulate (changes D2's test):** a call whose stamped scope is owned by
  `reserved:prefrontal-core` with `kind` `head`, `worker` or `ephemeral`. Testing `agent_id` alone
  would miss every worker, because worker scopes carry no `agent_id`. A call with no stamp, or a
  scope from any other owner, belongs to AFT alone, and its plugin asks in the host UI as it does
  without Alfonso. That standalone path is untouched by this design: when OpenCode or Pi is used on
  its own there are no Cingulate asks, and Alfonso use and standalone use stay separate. If
  Cingulate is not running,
  AFT denies an Alfonso agent's call with `access_unavailable`; it never falls back to a host
  prompt for a worker, since nobody would answer it.

## Routing (keeps D1)

- **Worker ask:** the answerer is the worker's head. The user sees it only if the head escalates.
- **Head ask:** the answerer is the user, in the Alfonso apps only: the phone now, the desktop
  app later. There is no host-UI prompt for an Alfonso agent's ask, whatever harness the head runs
  in.
- **Escalated worker ask:** goes to the user in the Alfonso apps, as a head ask does.

## Kinds

- **`tool_permission`**, requested by `reserved:aft` only. Body: `call_key`, `tool`,
  `operation` (`read`, `write` or `exec`), `paths` (at most 16 absolute paths), `command`
  (exec only, at most 600 characters), `reason` (one line, at most 160 characters), `args_hash`,
  optional `schema_pin`, `pattern` (what AFT would store for a scope grant) and, on a worker's
  ask, `head_policy`: `allow` when the head already holds the permission itself, either because
  AFT's policy allows the operation for the head's scope or because the head holds a grant covering
  it, and `not_held` otherwise. Options are fixed: `allow_once` and `allow_scope` (effect grant)
  and `deny` (effect decline); `deny` is the default answer. Nothing expires, so there is no expiry
  outcome. An option the current answerer may not give is left out.
- **`macos_permission`** (keeps D9), requested by `reserved:aft` only, informational: no options,
  nothing waits on it. It names the folder, the program to grant and the steps in System
  Settings. One open card per (folder, program): a repeat request returns the open card. AFT
  closes it with `consent.withdraw` when it sees the folder readable again. Refused on platforms
  other than macOS.

## Answers (keeps D5)

| Answer | Effect for AFT | Who may give it |
|---|---|---|
| `allow_once` | grant for this `call_key`/`args_hash` only | the worker's head, only when `head_policy` is `allow`; the user |
| `allow_scope` | grant for the asking scope until that scope epoch ends | the worker's head, only when `head_policy` is `allow`; the user |
| `deny` | decline, with an optional one-line note to the asker | the head; the user |
| `escalate` | moves the ask to the user | the head only |

A head that does not hold the permission itself can only deny or escalate, so it can never pass
on more than it has. A grant always applies to the asking session's scope. A user's answer on an
escalated ask, "for this task" included, applies to the worker's scope only, never the head's.

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
  the asking worker's stamp, and only with the answers the table allows for that `head_policy`.
  Core is already the authority that
  assigns agent identity, so relaying a head's answer gives it nothing new.
- **User in the Alfonso apps:** the existing phone rule (`consent.answer` from `reserved:callosum`
  with a harness starting `fed-phone:`), plus a new restriction: only while the ask is in its user
  leg. The desktop app answers through the same rule when it ships. AFT's plugin never shows a
  host prompt for an Alfonso agent's call.

The first answer to reach Cingulate's record wins (a compare-and-set in one store transaction).
Cingulate then withdraws every other surface, and a late answer is refused with the settled answer.

## AFT's side of the record

- **Waiting:** `consent.await {elicitationId}` is a long-poll that returns when the card settles
  or at the caller's wait bound. It reads only, so AFT may re-await at any time.
- **Withdrawing:** when a held call ends without an answer (client cancelled, route closed, a run's
  scope ended, or the asker gone), AFT calls `consent.withdraw {elicitationId}`. The raising
  principal may withdraw a card, and `reserved:prefrontal-core` may withdraw a `tool_permission`
  ask whose recorded asker scope it owns. Cingulate checks only that ownership. Prefrontal Core
  withdraws only once a worker run is terminal (delivered, failed or cancelled), never while
  the worker is parked waiting on the ask. The record moves to `access_withdrawn`.
- **After an AFT restart:** AFT re-sends `consent.request` with the persisted `call_key`; the same
  `call_key` returns the same ask, and AFT awaits or withdraws it.

## Waiting and waking (replaces D6)

- **No timers.** An ask stays pending until it is answered, escalated, withdrawn, or its head is
  gone. Neither leg expires. This is how a worker's question to its head already works.
- **Holding the call:** AFT holds the call for its normal reply budget (at most 100 s), so an ask
  answered quickly never interrupts anything. If the ask is still open then, AFT ends the call with
  `access_pending`, whose text tells the agent plainly: it is waiting for permission, it should end
  its turn, and it will be resumed with the answer. Tool deadlines stay short, so a stuck call is
  still caught at the normal bound.
- **Ending a turn is not finishing:** this is the same parked path a worker's question to its head
  already uses. Core records the worker as awaiting an answer when it receives the `head_ask`
  notice (which names the asking session). While that record is open, core's completion gate
  answers "blocked", not "completed", and its stall and nudge detectors stand down, so a mason
  that ends its turn on `access_pending` is never treated as done. When the ask leaves its pending
  states (answered, refused or withdrawn), the `settled` notice clears the record and steers the
  result into the same worker session, which resumes the turn; the agent then runs the call again,
  and AFT matches `allow_once` to the same `args_hash`. A head's own ask parks the same way.
- **Considered and left out:** declaring an 11-minute `reply.max_ms` on every tool that can ask,
  so the call blocks for the whole leg. It would make a stuck edit, write or bash call take 11
  minutes to be caught, and the parked path already keeps a waiting worker from finishing. A
  mid-call deadline extension would avoid that cost but is a new contract feature across every
  caller; it is not needed.
- **Waking a head for its worker's ask:** Cingulate calls core's `session.notice` (core change 2
  below). No phone push goes out for the head leg. While the ask stays open, core reminds the
  head periodically, the way it reminds a head of a worker's unanswered question today.
- **Busy heads:** a head in the middle of a long tool call sees the notice only at its next step
  (Pi steers between calls, OpenCode at the next step). Since nothing expires, a busy head
  delays the worker but never turns the ask into a refusal.

## What core builds

1. **Carrier listing:** `reserved:aft` listed as a carrier toward `cingulate` on head and worker
   scope records, unless today's carrier list already covers it.
2. **`session.notice`**, admitted only from `reserved:cingulate`, with no answer path. Its exact
   wire shape is in "The `session.notice` wire" below. Core deduplicates on
   `(elicitation_id, purpose)`, so a retry never wakes twice while each step of one ask still
   wakes once. Core renders the text itself and escapes every field, so a worker's `reason`
   cannot inject instructions into its head's session, and every notice reads like core's
   others. If the asker is idle when `settled` arrives (usual after `access_pending`), core
   starts a new run in the same session with the result, the same path a head's `work(prompt)`
   uses; a busy asker gets it as a steer at its next step. For an open `head_ask`, core reminds
   the head periodically until the ask leaves its head leg.
3. **The `acc_` arm of `work(prompt)`**, accepting only the four answers and relaying them as
   described above.

### The `session.notice` wire

Request body (core refuses unknown top-level keys and ignores unknown keys inside `fields`, so
Cingulate can add display fields without a core change):

```
{
  "elicitation_id": "acc_...",
  "purpose": "head_ask" | "user_ask" | "escalated" | "settled" | "withdrawn",
  "asker": { "scope_ref": "...", "scope_epoch": 3 },
  "head":  { "scope_ref": "...", "scope_epoch": 7 },   // worker asks only
  "fields": { ... }
}
```

`asker` and `head` are daemon scope refs and epochs, exactly as the daemon stamped them on
Cingulate's routes. Core resolves each to the session it holds, and treats an epoch older than
the session's current one as not live. `head` is present on every notice about a worker's ask
and absent on a head's own ask.

What each purpose does in core:

| purpose | Sent when | Core's effect | Must be live |
|---|---|---|---|
| `head_ask` | a worker's ask opens | park `asker` as awaiting an answer; wake `head` with the ask; remind `head` periodically | `head` and `asker` |
| `user_ask` | a head's own ask opens | park `asker` (the head itself) as awaiting an answer; no wake, since the phone carries it | `asker` |
| `escalated` | the head passed its worker's ask to the user | stop reminding `head`; `asker` stays parked | `asker` |
| `settled` | the ask got an answer or a refusal | clear the park; stop any reminders; deliver the result to `asker` | `asker` |
| `withdrawn` | the asker's call ended without an answer | clear the park; stop any reminders; deliver nothing | none (a no-op is fine) |

`fields` (all on `head_ask` and `user_ask`; `outcome`, `answered_by` and `note` on `settled`;
`note` on `escalated`; nothing required on `withdrawn`):

| field | type |
|---|---|
| `tool` | string, at most 64 characters |
| `operation` | `read` \| `write` \| `exec` |
| `paths` | array of absolute paths, at most 16 |
| `command` | string, at most 600 characters; `exec` only |
| `reason` | string, at most 160 characters |
| `head_policy` | `allow` \| `not_held`; worker asks only |
| `answers` | array of `allow_once` \| `allow_scope` \| `deny` \| `escalate`: what the head may answer now |
| `outcome` | `allow_once` \| `allow_scope` \| `deny` \| `access_head_gone` \| `access_no_head` |
| `answered_by` | `head` \| `user`; absent for a refusal |
| `note` | string, at most 160 characters, optional |

Reply: `{"ok": true}`, including for a repeated `(elicitation_id, purpose)`. Refusals by name:

- `unauthorized_caller`: not `reserved:cingulate`;
- `invalid_params`: a schema violation, naming the field;
- `unknown_session`: core never held that scope ref;
- `session_not_live`: the session ended, finished, or the epoch is stale.

What Cingulate does with each refusal:

- **`session_not_live` or `unknown_session`:**
  - for `head_ask`, the ask becomes `access_head_gone`;
  - for `settled`, the settlement is recorded as unclaimed;
  - in every case, no retry.
- **`unauthorized_caller` or `invalid_params`:** logged as a defect, with no retry.
- **Transport failure or core unavailable:** retried with backoff. This never blocks the ask's
  own state.

## Refusals (keeps D8)

Every refusal has a name, and none grants: `access_no_head` (no parent head, or its scope not
live), `access_head_gone`, `access_not_placed` (no surface can answer now),
`access_withdrawn` (the call was cancelled), `access_declined` (with the note),
`access_unavailable` (Cingulate unreachable, returned by AFT). A head answering an ask that is not
its own worker's, or allowing one when `head_policy` is `not_held`, is refused to the relay and the
ask stays pending. The full code list and wire meanings are in the [tool-permission outcomes contract](../contracts/tool-permission-outcomes-v1.md).

## State machine

One record per ask, in Cingulate's store:

```
request ─┬─ worker ask ─▶ [pending:head] ─escalate─▶ [pending:user] ─first answer─▶ [settled]
         │                      │ answer ─────────────────────────────────────────▶ [settled]
         └─ head ask ──────────────────────▶ [pending:user] ─first answer─▶ [settled]
any pending state ─ head gone / call withdrawn ─▶ [refused: access_head_gone | access_withdrawn]
(no timed transitions: a pending ask waits until one of the above happens)
```

An asker whose call returned `access_pending` stays parked in every pending state and is resumed
when the record leaves them.

A second request with the same `call_key` returns the same ask; an asker never re-asks.

## Audit

Every settled record keeps the asker, its head, the scope ref and epoch, the request fields,
the surfaces offered, every answer attempt with surface and time (losers included), any
escalation note, and the final state. Standing grants stay with AFT (`grants.list`,
`grants.revoke`), as for every provider.

## Rulings (Ufuk, 2026-10-11)

1. A worker's ask goes to its head first. The head may allow (once or for the scope) whenever it
   already holds the permission itself, by AFT's policy or a covering grant; otherwise it may only
   deny or escalate. `head_policy` reflects covering grants as well as policy.
2. No host-UI surface for an Alfonso agent's ask: it is answered only in the Alfonso apps.
   Standalone use (no Alfonso scope) stays AFT alone with its own host prompt.
3. No timers: an ask waits until answered, escalated, withdrawn or its head is gone, and core
   reminds the head periodically.
4. A user's "for this task" on an escalated ask applies to the worker's scope only.
5. Deferred, not v1: auto-approval on silence based on how reversible the action is. Nothing is
   built for it now; the record keeps every field such a rule would need (operation, paths,
   command, the time the ask was raised), so it can be added later without a format change.
