# Tool-permission outcomes v1

This contract lists the terminal outcomes and request refusals for `tool_permission` and `macos_permission` asks. Only `allow_once` and `allow_scope` grant. Every other code listed here, including `deny`, grants nothing and is a terminal refusal that is never retried. A reader that encounters an unknown code treats it the same way: a terminal refusal, never a grant or a reason to retry.

Harness-only means a table row emitted by `thalamus-harness` or `aft`, but not by `cingulate`. Cingulate sends `session.notice` messages to the Prefrontal component (`reserved:prefrontal`), which waits for the ask's result and returns it to the requesting agent. A settled notice puts the outcome in `fields.outcome`. These messages never contain harness-only codes. Before adding such a code to them, update every component that reads `session.notice` so it recognizes the code. `access_withdrawn` also covers a cancelled call, provider withdrawal, or route close in the Thalamus harness. For both Cingulate and the Thalamus harness, `elicitation_invalid_request` is used for any request field that breaks the request limits.

A code that can arise both ways lists both stages. Its vectors are `<code>.json` for the request-time refusal and `<code>.outcome.json` for the `consent.await` outcome; a code with one stage has only `<code>.json`, in that stage's shape.

| code | when it happens (one line) | who emits it | when it arises |
| --- | --- | --- | --- |
| `allow_once` | The person chose to allow this invocation only. | `cingulate` | `outcome` |
| `allow_scope` | The person chose to allow within the requested scope. | `cingulate` | `outcome` |
| `deny` | The person explicitly declined the ask. | `cingulate` | `outcome` |
| `access_no_head` | A worker's ask is refused if it does not identify the session that started the worker, or if that session has ended. | `cingulate` | `request` |
| `access_head_gone` | A pending head-side ask settles refused after its supervising head is confirmed gone. | `cingulate` | `outcome` |
| `access_asker_gone` | A pending ask on any answer leg settles refused after its stamped asking session is confirmed gone. | `cingulate` | `outcome` |
| `access_not_placed` | The harness cannot show the ask because there is no person or prompt surface: refused at request time when the host is known up front not to ask, or settled from `consent.await` when that is found only later (no prompt surface attached, it detached mid-wait, or a host rule resolved the prompt without a person). Cingulate's design reserves this meaning, but Cingulate does not emit it today. | `thalamus-harness` | `request`, `outcome` |
| `access_withdrawn` | The asker withdrew a pending ask. | `cingulate`, `thalamus-harness` | `outcome` |
| `access_declined` | The person chose `deny`; this is the wire code for outcome `deny` and may carry the person's note. | `cingulate` | `outcome` |
| `access_unavailable` | Cingulate cannot tell whether the parent session is available, or AFT cannot reach Cingulate. | `cingulate`, `aft` | `request` |
| `access_not_alfonso` | The request has no scope identity attached by the subc daemon, or the attached scope does not name `reserved:prefrontal` as its owner. | `cingulate` | `request` |
| `access_macos_only` | A `macos_permission` ask arrived on a non-macOS runtime. | `cingulate` | `request` |
| `access_informational` | `consent.await` was requested for a `macos_permission` ask, which has no waiting call. | `cingulate` | `outcome` |
| `access_wrong_leg` | An answer request arrived on a surface other than the ask's pending answer leg. | `cingulate` | `request` |
| `elicitation_invalid_request` | The request uses an unsupported permission operation or a request field breaks the limits on either server. | `cingulate`, `thalamus-harness` | `request` |
| `elicitation_not_found` | The ask does not exist or is not available on the requested surface. | `cingulate` | `request` |
| `elicitation_not_owner` | The requester does not own the ask. | `cingulate` | `request` |
| `elicitation_requester_refused` | A requester tried to detach a tool-permission ask, which does not support detach. | `cingulate` | `request` |
| `access_dismissed` | The person closed the prompt without choosing, and the host can distinguish that from “no.” | `thalamus-harness` | `outcome` |
| `access_declined_or_dismissed` | The host returns the same negative for “no” and for closing, so they cannot be distinguished; it is never remembered as a deny. | `thalamus-harness` | `outcome` |
| `access_unrecognized` | The person typed free text instead of choosing; `note` carries it, cut to 160 characters, and it is never consent. | `thalamus-harness` | `outcome` |
| `access_stale` | The person chose allow, but `consent.still_current` returned `not_current`, failed, or timed out. | `thalamus-harness` | `outcome` |
| `access_no_answer` | AFT's held call ended without an answer; it is never a grant and is never resumed. | `aft` | `outcome` |
| `access_use_cingulate` | A request reached the harness under a route stamp other than its own `reserved:thalamus-harness` scope. | `thalamus-harness` | `request` |
| `access_call_unknown` | `call_id` is missing or is not an in-flight call from this session to this provider. | `thalamus-harness` | `request` |
| `access_conflict` | A reused `call_key` has different fields. | `thalamus-harness` | `request` |
| `access_queue_full` | The session already has its limit of four waiting asks. | `thalamus-harness` | `request` |
