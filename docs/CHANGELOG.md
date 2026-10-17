# Changelog

## 0.9.0 (ck-cingulate)

- Accept execution reports after requester acknowledgment while the answered card
  is retained; requester get, await and answers still hide acknowledged cards.
- Backstop retained, answered generic consent cards without a report when their
  stamped asker session has definitely ended. Record `approved_not_executed` with
  reason `session_ended`, using bounded maintenance passes and conservative scope
  liveness. Unknown, unsynced and unreachable scopes are left alone.
- Preserve requester report replacement, including clearing an omitted reason.
  The backstop never overwrites any report; a requester report always replaces
  the backstop, including when they race.
- Pin requester and phone backstop projections in `consent-session-ended-v1`
  without changing existing corpus bytes. No store schema change.

## 0.8.0 (ck-cingulate)

- Make `expires_in_ms` optional and advertise `consent-no-expiry/v1`. Without a
  lifetime, cards remain pending until answered or withdrawn; existing explicit
  lifetimes retain their bounds and fail-closed behavior. Migration 6 makes expiry
  nullable without rewriting cards, settlements, revisions or answer cursors.
- Order every pending card oldest first by creation time then id, including
  permission asks. Serve JSON object cursors `{created_at_ms, elicitation_id}`;
  accept the prior expiry-keyed object for one release by resolving its card.
- Accept an optional open-set `reason` code with `approved_not_executed`, store it
  with the execution report, and project `execution_reason` to requesters and
  `executionReason` to the phone. Refuse malformed or inappropriate reasons.
- Add served no-deadline, mixed-list and execution-reason vectors while retaining
  all existing corpus bytes.

## 0.7.1 (ck-cingulate)

- Add optional `single_use` to generic consent requests and advertise
  `consent-single-use/v1`; providers must check the running module's
  `catalog.list` capabilities before sending the field because older Cingulate
  versions reject unknown request fields.
- Include `requester_ref` on core `consent.outcomes` records and advertise
  `consent-outcomes-late/v1` for outcomes that report `late: true` when settlement
  follows a committed detach. Keep `requester_ref`, `single_use` and `late` off
  phone projections and add dedicated served vectors; existing vector sets remain
  byte-identical.

## 0.7.0 (ck-cingulate)

- Keep settled generic consent cards readable through phone `consent.get` after
  requester acknowledgment until the existing 24-hour retention prune. Keep
  requester get, answer feeds, pending lists, and pruning behavior unchanged, and
  continue to refuse phone answers on settled cards with `conflict`.

## 0.6.1 (ck-cingulate)

- Keep permission cards readable through phone `consent.get` after they have
  been presented to the user.
  Project user answers as `answered` with the selected choice and original
  options; project other endings as `withdrawn` without a user choice. Pending,
  requester, core and head projections and first-answer-wins are unchanged.

## 0.5.8 (ck-cingulate), 0.5.8 (cingulate-vectors)

- Let the head that escalated a worker tool-permission ask deny it while the user
  has not answered. Require the exact recorded escalator identity, never accept
  a head grant after escalation, and retain first-settlement-wins, phone card
  retraction and durable answer audit. Add served vectors without changing
  existing scenario bytes.

## 0.5.7 (ck-cingulate), 0.5.7 (cingulate-vectors)

- Let Prefrontal Core withdraw tool-permission asks whose recorded asker scope it
  owns when a worker run finishes, fails or is cancelled. Audit the withdrawing
  principal and add served owner, non-owner and already-settled withdrawal vectors
  without changing existing corpus bytes.

## 0.5.6 (ck-cingulate), 0.5.6 (cingulate-vectors)

- Add phone-only human names for Cingulate-admitted requester principals
  (`reserved:basal`, `reserved:aft`, `reserved:plexus`, `reserved:cerebellum`,
  `reserved:broca`, `reserved:entorhinal`, `reserved:prefrontal-core`); omit
  `direct` and unknown ids. Preserve raw `requesterLabel` and requester/core
  projection bytes. Regenerate affected phone vectors and pins.

## 0.5.5 (ck-cingulate), 0.5.5 (cingulate-vectors)

- Add served phone vectors for Basal's three flow-decision approval cards.
  Preserve the supplied request bytes and pin the phone-visible choices and labels.

## 0.5.5 (ck-cingulate), 0.5.4 (cingulate-vectors)

- Serve escalated permission asks to the phone with state `pending`. Shipped
  phones keep and answer only cards whose state is exactly `pending`, so the
  `pending:user` state hid them. The requester and core projections still report
  `pending:user`.

## 0.5.4 (ck-cingulate), 0.5.3 (cingulate-vectors)

- Make worker tool-authorization requests decodable on existing phone builds with
  plain-text prompts, human option labels and facts. Group worker cards under the
  supervising agent's identity and display name. Add the same descriptive fields
  to informational macOS folder-permission cards without adding answer buttons.
  Preserve requester projections and decline defaults; publish the served
  `access-escalated-phone-*` fixtures for escalation and a phone's one-time grant
  without changing existing requester or other corpus bytes.

## 0.5.3 (ck-cingulate), 0.5.2 (cingulate-vectors)

- Add the optional `source` and `dry_run_text` human-readable card fields for
  installation and package approvals, with closed shape, size and plain-text
  validation. Preserve them in requester and phone readbacks and advertise the
  `consent-card-text/v1` capability; add real-handler served vectors.

## 0.5.2 (ck-cingulate, cingulate-vectors)

- Allow `reserved:broca` to raise generic consent requests only; restrict its
  follow-up operations to the owner who raised each request. Add served vectors
  for Broca requests and the refusal of a non-consent request.

## 0.5.1 (ck-cingulate)

- Expire flow-decision cards without recording a selected choice, matching the
  consent contract for every card kind.

## 0.5.0 (ck-cingulate, cingulate-vectors)

- Serve the operator's permissions inventory by discovering `permissions/v1`
  catalog capabilities and concurrently reading each provider. Preserve named
  unreachable/unsupported gaps, sorted module ids, RFC 8785 SHA-256 versions and
  conditional reads. A catalog outage refuses rather than claiming no grants.
- Route revokes once to the grant's provider, preserve provider refusal codes,
  and report lost replies as `module_unreachable` with an unknown revoke outcome.
  Close each per-call provider route on completion or cancellation, without
  sharing routes across calls. Real-daemon tests pin cleanup after repeated
  reads and a refusal, and consent/health responsiveness during a provider timeout.
  Admit phone mutations and phone/simulator queries from stamped Callosum routes,
  plus the local `direct` route; leave consent admission phone-only.
- Pin additive real-handler permissions vectors without changing existing vector
  bytes, and align the generic-consent operation list with its three reserved
  requesters, including Entorhinal.

## 0.4.0

- Allow the Plexus requester to create and manage its own generic `consent`
  cards. Cingulate takes each card's requester and subject identity from the
  daemon route stamp, not request params, and refuses other kinds. Add its
  consent-request and non-consent-refusal examples to the v2 corpus.

## 0.3.0

- Pin Cerebellum's generic-consent-only admission and owner-scoped requester
  operations with real-handler coverage; identity remains daemon-attested.
- Add optional printable-ASCII `requester_ref` correlation for generic consent.
  Preserve it in durable requester records and feeds, exclude it from phone
  cards, and keep it independent of pending-card deduplication.
- Publish additive requester v2 vectors without changing any v1 bytes. Existing
  JSON records accept the absent field, so no store schema migration is needed.

## 0.2.0

- Add AFT-only tool permission asks, worker-to-head routing from daemon stamps,
  policy-limited core relays, escalation to the phone, first-settlement-wins and
  durable answer-attempt audit. Neither the supervising-session nor the user
  answer path expires.
- Add informational macOS folder permission cards, deduplicated while open.
- Add restart-safe call-key replay, principal-only withdrawal and read-only,
  re-awaitable waits with caller bounds.
- Track parent scope liveness through the daemon's `scope.describe`; route loss
  and unknown liveness never imply a head has ended.
- Deliver five structured core notices through a durable, deduplicated outbox.
  Retry transport failures without blocking mutations; record unclaimed results
  and treat definitive missing heads as named refusals.
- Migrate version-3 stores forward to version 4 without rewriting existing cards.
  Preserve existing contracts and corpora; publish requester and phone v2 plus
  a SHA256-pinned served permission corpus.
