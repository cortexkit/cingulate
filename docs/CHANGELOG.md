# Changelog

## Unreleased

- Add the optional `permissions/v1` row member `ends_with_holder` and the
  checksum-pinned `provider-list-ends-with-holder` sample for holder-bound grants
  with and without an expiry. Contract and test data only; no runtime behaviour
  change.

## 0.20.0 (ck-cingulate), 0.11.0 (cingulate-decode), 0.9.0 (cingulate-store), 0.12.0 (cingulate-vectors)

- Add closed, bounded `config_plan` apply/restart scope, affected projects,
  policy changes and launch manifests. Allow zero field changes when a policy
  or program row records the change; keep unknown sections Decline-only.
- Add the `layer_file` policy change for settings-file edits with no effective
  setting change, carrying only absent/file-hash facts.
- Bind config-plan answers to `expectedRound` (omitted means 0) inside the
  recording transaction. Return typed `round_changed` results for stale rounds
  and `statement_reused` for previously recorded statement digests, while
  preserving exact current-round retries. Reuse the existing audit digest
  column; no migration is needed.
- The phone must read and render every new member in the checksum-pinned
  `config-plan-v2` samples before Callosum files them. Preserve the original
  `config-plan-v1` payload unchanged.
- Persist all sections in the existing record JSON and project them unchanged
  across phone/requester reads and reopened rounds. No schema migration is
  needed. Historical cards without an apply summary remain readable and
  Decline-only.

## 0.19.0 (ck-cingulate), 0.10.0 (cingulate-decode), 0.8.0 (cingulate-store), 0.11.0 (cingulate-vectors)

- Add `consent-config-plan/v1`: Callosum-only, fail-closed settings approval
  cards with typed changes and private requester bindings. Subjects and pushes
  describe this computer, never an agent or session.
- Carry bounded phone statement/signature bytes unchanged to Callosum, without
  parsing or verifying them. Record only statement SHA-256 and decoded length
  in the additive config-plan audit table (migration 12).
- Add sequence-checked `consent.report_unverified` to reopen an unexpired card
  atomically, invalidate its old current answer and push it again. Preserve
  person lifecycle outbox history with round-aware uniqueness and delivery order.
  Expose the same nonzero `round` in phone and requester records so a reopened
  card supersedes a final copy of the same id; omit it on the initial round.
  Retain prior
  answer audit metadata; each new answer increments `answer_seq`.
- Add reader-first phone/requester contracts and checksum-pinned, real-handler
  samples in `test-vectors/config-plan-v1/`.

## 0.18.4 (ck-cingulate)

- Add fixed, log-only refusal reasons to distinguish invalid requests, including
  agent and console mismatches. Refusal messages and statement bytes remain
  private, and caller replies are unchanged.

## 0.18.3 (ck-cingulate)

- Install a log subscriber at start, writing INFO and above to stderr, which the
  daemon captures as the module's log. Without one, every log line, including
  the per-refusal line from 0.18.2, was dropped.

## 0.18.2 (ck-cingulate)

- Log one line per refusal: method, refusal code, the caller's daemon-stamped
  principal and scope, the elicitation kind and id, and vault request ids.
  Request content and refusal messages are never logged.

## 0.18.1 (ck-cingulate)

- Accept worker secret-use statements naming the agent in the daemon's
  `on_behalf_of_agent` route stamp, as well as null. For non-worker scopes,
  statements must still exactly match the stamp's `agent_id`.

## 0.17.0 (ck-cingulate), 0.8.0 (cingulate-decode), 0.9.0 (cingulate-vectors)

- Advertise `consent-card-explanations/v1`. Accept optional `explanation` and
  `changes` author text on package and flow-install approval cards, with
  core-supplied labels of up to 64 Unicode scalar values and text of up to
  8,192 UTF-8 bytes. Refuse empty values and control characters U+0000–U+001F
  and U+007F–U+009F other than newline, naming the affected field.
- Persist both objects verbatim in existing card JSON and return them on get
  and pending-list readbacks; omit absent objects and keep author text out of
  push pointers and the durable notice outbox. No schema migration is needed.
- Add checksum-pinned, handler-generated phone and desktop samples, decoder
  boundary tests, restart/old-record compatibility and notice privacy coverage.

## 0.16.2 (ck-cingulate), 0.7.1 (cingulate-decode)

- Accept local and forwarded `ck-secret-use-statement/v3` and
  `ck-secret-flow-statement/v3` statements for extended API-key rules and login
  origins. Keep closed shapes, enforce v3 kind/shape agreement, and retain the
  existing forwarded-asker grammar and serving-vault exclusion.
- Refuse legacy/extended shape and version mismatches. Flow statements reserve
  the v3 statement tags for extended scopes and the v2 tags for forwarded
  requests, matching the vault contract. Local legacy flow ids remain unchanged.
- Fix flow cards accepting statements from different serving vaults: every
  statement must name the same secret home, for v1, v2 and v3 alike.
- Pin both claustrum `secret-multi-host-vectors-v1` statement samples byte for
  byte and cover all sixteen approval-required cases through filing, phone
  projection and answer binding. Envelope handling, v1/v2 vectors and existing
  card wording are unchanged; Cingulate relays scopes rather than rendering them.

## 0.8.4 (cingulate-vectors)

- Added the `permissions/v1` provider `provider-list-grant-sources` vector:
  four worker grants with `person`, `head_grant`, `head_policy` and `unknown`
  sources. The unknown-source grant's time is bounded by an earlier store
  migration; the other grant times are exact.
- Linked the optional `source` and `granted_at_bound` fields to the vector and
  refreshed the corpus checksums and pins. Other generated corpora remain
  byte-identical.

## 0.8.3 (cingulate-vectors)

- Added `permissions/v1` provider grant/denial listing and federated precedence
  vectors for standing denials, with the decision-table schema and refreshed
  checksum pins. Other generated corpora remain byte-identical.
- Added a test-only four-step precedence evaluator and aggregator coverage for
  unchanged `effect` relay and effect-sensitive listing versions. No
  `ck-cingulate` production code changed.

## 0.16.1 (ck-cingulate), 0.8.2 (cingulate-vectors)

- Display daemon-stamped remote agent and flow names beside their ids on
  federated consent cards and permissions labels. Persist valid names in card
  subjects, including phone and watch projections; drop invalid display text
  without refusing the caller. No name enters card ownership or grant matching.
- Pin subc-client-rs 0.38.2 and subc-protocol 0.36.2 (and the required transitive
  subc-control 0.38.2). Preserve existing unnamed wording byte for byte.
- Add the handler-generated named federated permissions sample, refresh corpus
  checksums, and cover renames, omitted names, distinct ids and unsafe names.

## 0.16.0 (ck-cingulate)

- Adopt exact subc-client-rs and subc-protocol 0.36.0 pins and declare
  `scope-federated/v1`. Consent subjects and operator device classes come from
  federated daemon stamps, never local agent resolution or harness text.
- Keep carried federated cards collectable after route loss and scope recreation
  by the same admitted provider and canonical paired subject. Local requester
  ownership remains scoped by owner, reference and epoch. Run cards retain one
  immutable, recoverable answer per elicitation and offer only single-use consent.
- Add computer-first remote card wording, federated permissions holder selectors,
  provider liveness rules and handler-generated reader vectors. Cingulate still
  holds no standing grants and relays only rows supplied by providers.

## 0.15.3 (ck-cingulate)

- Fill absent module-holder labels in `permissions.list_all` from every registered
  module's catalog `display_name`, preserving provider labels and other holder
  kinds. Names enter the snapshot before hashing, so renaming a holder changes
  the conditional-read version. Add a handler-generated three-row app vector.
- Upgrade subc-client-rs to 0.34.3 and subc-protocol to 0.34.5 (with transitive
  subc-control 0.35.1). Keep handle requests for revoke: only trusted, opted-in
  `route_busy` refusals may retry, never stale channels or uncertain outcomes.
  Lost replies, including the SDK's new completion-failed `OutcomeUnknown`,
  still report `module_unreachable`.

## 0.15.2 (ck-cingulate)

- Pending permission asks now settle as `access_asker_gone` when the daemon
  confirms the stamped asking session has ended, including asks already on the
  user surface. Settlement retracts phone cards and uses the existing notice
  outbox. Asker checks are paged, time-bounded and backed off on uncertain
  liveness; a live or unreachable session never grants or settles an ask.

## 0.15.1 (ck-cingulate), 0.8.1 (cingulate-vectors)

- Presence summaries and routing now come from `cortexkit-presence` 0.1.0 on
  crates.io, which core also uses, so both read one copy of the rules. The
  in-repository `cingulate-presence` crate is removed. Its code moved
  unchanged, and every presence sample regenerates byte for byte, so the
  `presence-v1` folder and its checksums are untouched. Its README still names
  the old crate, which is where those bytes were first generated.

## 0.15.0 (ck-cingulate), 0.1.0 (cingulate-presence), 0.8.0 (cingulate-vectors)

- Apps can report where the person is through `presence.report`. Reports replace
  the previous report for that phone or local app, expire within two minutes,
  and stay only in memory. The daemon route decides who may report and the
  device kind; only a local app under the development switch names itself. No presence data reaches the consent store, outbox or backups.
- Core and local operator apps can read `presence.get` or stream
  `presence.watch`. Active phones take precedence over computers. Watched chats
  merge inline item kinds and newest report times across active apps. Changes
  coalesce for 250 ms; timestamp-only refreshes do not emit a frame. Expiry uses
  the nearest deadline, not polling.
- The new pure `cingulate-presence` crate shares summaries and delivery routing
  with core, including recent inline suppression. Consent card streams are
  unchanged. Production-generated, checksum-pinned samples cover the contract.
- Upgrade the subc SDK to 0.33.1 and protocol to 0.34.0 so verified operator-app
  stamps reach the handler; migrate bind constructors to harness/session and
  the real-daemon fixture to daemon 0.39.0. `cingulate-decode` is unchanged.
- Declare `bind-no-project/v1`. Cingulate takes a project only from a session
  scope's stamp, never from the connection, so the daemon no longer fills a
  placeholder folder on routes to it. Cingulate's own routes send no folder,
  as before: the daemon fills `/` for targets that haven't declared
  `bind-no-project/v1`, the value Cingulate used to send itself.

## 0.14.0 (ck-cingulate), 0.7.0 (cingulate-decode), 0.6.0 (cingulate-store), 0.7.0 (cingulate-vectors)

- Retire rich UI approval cards. Installing a tool provider is now the consent
  for its rich UI (the operator's decision of 31 October 2026), so there is no
  separate rich UI approval. Remove the `rich_ui_approval` kind, its requester
  admission, approval-specific answer validation, device visibility,
  notifications, the revocation operation and the advertised capability. There is
  no compatibility handling: a request naming the retired kind meets the ordinary
  requester admission and request validation, so an admitted requester gets
  `elicitation_unknown_kind` and Occipital, no longer admitted, gets
  `elicitation_requester_refused`. Unknown client feature tokens are still
  ignored, so released phones can continue sending `rich-ui-approval`.
- Migrate the store schema from 9 to 10 in one forward migration: drop
  `rich_ui_device` from records and watch history and remove the `revoked`
  state. Remaining records, answer cursors, secret-use visibility and watch
  history are preserved. The migration converts nothing: it requires that the
  store holds no `revoked` row and no rich UI approval payload, pruned
  tombstones included. A store that does fails the migration and Cingulate
  doesn't start. Both production stores were checked on 31 October 2026 and
  hold neither.
- Remove the rich UI vector generator, support, tests and corpus from the working
  tree. The published `rich-ui-approval-v1` tag keeps them. Remove the
  corresponding requester and phone contract sections. Decoder public types
  change incompatibly. On 31 October 2026 no other CortexKit repository depended
  on `cingulate-decode`, and it had never been published to crates.io.

## 0.13.5 (ck-cingulate)

- Notify core about person cards filed without a session scope too, such as the
  install cards core files for its own flows. Their `escalated`, `settled` and
  `withdrawn` notices omit `asker` and `head`; everything else is unchanged, and
  scoped cards' notices keep their exact bytes. Until now these cards queued
  nothing, so they never pushed to the phone. Pending ones get their notice once
  on startup. Requires a core that accepts a notice without `asker`.

## 0.5.3 (cingulate-store)

- Make a person-card notice's `asker` optional and omit it for cards with no
  stamped scope. Scoped bodies are byte-identical to 0.5.2.

## 0.13.4 (ck-cingulate)

- Notify core when a phone-answerable card with a daemon-stamped asker is filed,
  answered, withdrawn or expired, so consent, secret-use, flow-install and rich UI
  cards can push to the person. Include only the attested subject and routing
  metadata, never card text or vault statements. Console-only cards do not push.
- Leave unscoped card filing unchanged and log its id and attested principal once,
  without request text; those cards have no session to address for a core notice.
- Deliver pending cards from before the upgrade once on startup through the
  existing durable outbox. Permission-ask notice bytes and routing are unchanged.
- Attribute a worker's notice to a head only when the daemon confirms the
  same-owner parent link. Do not start empty delivery tasks that keep a stopped
  module's store lease alive, or open a backfill write transaction with no missing
  notices.

## 0.5.2 (cingulate-store)

- Queue person-card notices in the filing and settlement transactions, supersede
  undelivered opening notices, and backfill pending cards idempotently on open.
  Keep the worker's attested parent privately for notice routing. No schema change;
  older binaries can still open the store.

## 0.6.3 (cingulate-vectors)

- Adapt the record test identity to the optional attested parent field. Existing
  phone projections and permission notice goldens remain unchanged.

## 0.13.3 (ck-cingulate)

- Show `<agent> in project <name>` on consent subjects when the filing route's
  daemon stamp carries a project id. Entorhinal resolves aliases before the name
  lookup; gone projects and unavailable reads show the raw stamped id. Card
  `scope` remains opaque filer text, and subjects without a stamped project are
  unchanged. Store and serve the same composed subject in the existing string.
- Cache the registry's project names for one minute. Cap the whole project-name
  lookup at 500 ms while running it alongside the agent-name lookup. A failed
  project lookup falls back to the stamped id without refusing the card; the
  project name is display-only and does not affect authorization.
- Opt into `scope-workdir/v1` so the daemon forwards `project_id` (and `workdir`)
  on bind stamps; this changes attribution only, not route admission. Upgrade
  protocol 0.32.5 → 0.33.5 and client 0.30.1 → 0.32.12 for the stamp, with control
  0.32.1 → 0.33.4 transitively. Test against daemon 0.38.0 (from 0.36.14) and
  transport 0.14.1 (from 0.13.0), matching their compatible dependency lines.

## 0.13.2 (ck-cingulate)

- Open ordinary operator cards to daemon-stamped `Principal::Direct` local
  programs during development with the code-only `DEVELOPMENT_LOCAL_OPERATOR`
  switch. Local programs can list, get, answer and watch cards without a device
  fingerprint. Permission answer audits retain the real principal `direct`.
  Set the switch to false before public release.
- Keep secret-use cards, flow installs carrying secrets and device-bound rich UI
  approvals hidden and unanswerable locally. Simulators, unverified callers and
  other reserved modules gain no operator authority.
- Preserve local callers' requester operations: `consent.get` selects the operator
  projection only for the `elicitationId` form, and permission requester operations
  keep their ownership checks. Live daemon coverage follows the development switch
  without granting a local caller phone identity.

## 0.5.1 (cingulate-store)

- Accept explicitly route-authorized operator answers on the permission user leg,
  preserving the real audit principal and optional device. Existing phone answers
  remain valid; head-leg and all other answer checks are unchanged.

## 0.13.1 (ck-cingulate)

- Accept v2 vault statements. When an agent or flow on another paired computer
  asks for a secret, the home computer's vault writes the usual statement plus
  `asking_member_id`, the computer that asked, and marks it
  `ck-secret-use-statement/v2` or `ck-secret-flow-statement/v2`. Cingulate now
  files and answers these on `secret_use` and flow-install cards exactly as it
  does v1: same bytes rule, same envelope check, same recorded fingerprint and
  expiry.
- Refuse, with the existing codes: a v1 statement carrying `asking_member_id`
  (even `null`); a v2 statement without it, or with anything other than
  `fed-computer:` and 32 lowercase hex characters; an asking computer equal to
  the serving vault's `member_id`; a version from the other statement family;
  and a card whose statements name different asking computers or mix v1 and v2.
- v1 statements, stored answers and every existing vector are unchanged. New
  corpora `secret-use-approval-v2` and `flow-install-secrets-v2` hold the
  vault's v2 vectors (Claustrum `af71c16`, copied byte for byte) and phone,
  filer and refusal samples generated by Cingulate's own handler.

## 0.13.0 (ck-cingulate)

- Advertise `flow-install-secrets/v1`: core may attach paired `flow_identity` and
  `secrets` fields to one `flow_install` card. Validate the vault's canonical
  flow statements, exact installed-version identity, unique request ids and
  version-only duration, leaving all other install fields opaque.
- Secret-bearing install cards use the existing supported-phone gate and
  envelope/answer recording. An install requires every statement exactly once,
  duration `version`, zero grant expiry and the answering phone's fingerprint;
  declines require no envelope. The card expires denied at the earlier filed
  or statement deadline, including when core omits its expiry.
- Add generated filing, phone, allow, decline and refusal vectors based on
  Claustrum `1ee946f` goldens. Existing corpora remain unchanged. No storage
  migration: reuse the JSON records, answer binding and secret visibility flag.

## 0.6.2 (cingulate-vectors)

- Extend the `permissions/v1` contract with grants held by an installed flow
  version (`holder.kind: "flow"`: `flow_id`, `flow_version`, `flow_install_id`,
  no agent id and no expiry, listed only in unfiltered reads) and with the
  optional `methods` row field limiting an API-key grant. A reader keeps a row
  whose holder kind it does not know and never shows it as operator-held.
- Add the matching `provider-list` and `provider-list-after-expiry` rows
  (invented ids and labels) and anchor `holder` with `.jcs` and `.sha256`. No
  runtime behaviour change: the aggregator already relays provider rows
  unchanged.

## 0.6.1 (cingulate-vectors)

- Add filer-side `secret_use` reply vectors in `test-vectors/secret-use-approval-v1/`
  for AFT and Cerebellum decoder builders, all produced from the real handler:
  filing, repeat filing, pending/allowed/partial/declined/expired awaits, and
  four filing refusals. No behaviour change.

## 0.12.0 (ck-cingulate)

- Add `secret_use` cards for vault agent-secret approvals, filed only by scoped,
  daemon-attested AFT and Cerebellum routes. Closed canonical vault statements
  bind the agent, request ids and earliest expiry; duplicate filings are
  idempotent and request ids cannot be reused for a different card.
- Advertise `secret-use-approval/v1`. Only fingerprinted Callosum phone routes
  with `secret-use-approval` can list, get, watch or answer these cards, including
  declines. Watch privacy survives payload pruning. Notifications contain only
  generic card pointers, never secret names, hosts or agent ids.
- Bind allows to the exact canonical envelope, answering device, statement
  digests and offered duration. Persist per-item allow/decline decisions and the
  envelope SHA-256 for matching the vault grant/audit; omitted items decline.
  Both choices refuse after the earliest request expiry. Cingulate does not
  verify signatures or replace the vault's approval checks.
- Release cingulate-decode 0.6.0 (closed statement/envelope decoding),
  cingulate-store 0.5.0 (migration 9, durable request-id reservations and answer
  binding), and cingulate-vectors 0.6.0 (vendored vault goldens and real served
  single/batch phone records with regeneration and refusal tests).

## 0.11.2 (ck-cingulate)

- Wire production agent status reads through Entorhinal's typed `agent.resolve`:
  retired and merged subjects refuse `consent_retired`; unknown identities and
  unavailable reads still show the raw agent id. Lookups have a 500 ms deadline,
  a bounded 60 s cache, and rate-limited, id-free failure warnings.
- Resolve generic and permission-card display names from Entorhinal's typed
  `agent.fleet_identity` roster, refreshing with its opaque token and once on a
  name miss. Unchanged replies retain the roster; full replies replace it.
- Still check retirement when a name read fails, reserving time for status within
  the lookup deadline. Roster-derived results expire with the roster rather than
  extending its 60 s live-status cache window.

## 0.11.1 (ck-cingulate)

- Read Callosum's `computer` device class: a `fed-computer:<fingerprint>` route
  answers a `rich_ui_approval` card bound to that fingerprint, as `fed-mac:` did.
  Both spellings are read until no computer runs a Callosum build from before
  the rename. Neither is the operator surface; that stays `fed-phone:` only.

## 0.11.0 (ck-cingulate)

- Add typed, fail-closed `rich_ui_approval` cards requested only by stamped
  `reserved:occipital`. Grants require an exact `bindDigest`; mismatches refuse
  `approval_digest_mismatch`, and other kinds reject the field as `invalid_params`.
- Gate phone list/get/watch/answer with `rich-ui-approval` and the intended
  device fingerprint. Featureless answers refuse
  `elicitation_rich_ui_approval_required`; wrong-device reads and answers behave
  exactly as unknown IDs. This kind never emits notifier pointers.
- Add owned, idempotent `consent.revoke {elicitationId}` for granted approvals,
  terminal `revoked` state and `revokedAtMs`. Withdrawal is unchanged.
- Migration 8 extends the state constraint while preserving answer cursors and
  triggers, and retains device visibility on change-log rows even after pruning.
  Record fed-route answering fingerprints for generic cards and permission attempts.
- Pin complete served states, feature gates and bound-answer/refusal samples in
  `test-vectors/rich-ui-approval-v1/` (removed in 0.14.0; kept at the
  `rich-ui-approval-v1` tag); document phone and requester contracts.

## 0.10.0 (ck-cingulate)

- Add `consent.watch_pending`, a live stream of phone-visible card changes that
  replaces polling `consent.list_pending`. It is declared with watch delivery in
  the manifest, admitted exactly like `consent.list_pending`, and streams
  `reread`, `changed` (the phone `consent.get` projection) and `removed` items
  with an opaque `{instance, version}` object cursor. A usable cursor replays
  every card changed after it, at its latest change, in order; a missing,
  foreign, future or compacted cursor starts with `reread`. Items over 192 KiB
  become `reread`. At most 16 watches are open (`watch_capacity` beyond); they
  end on unsubscribe, route close, module drain and connection end; a reader
  more than 256 changed cards behind gets `reread` and the watch ends.
- Migration 7 adds the change log: one row per committed change to a card or
  permission ask, written by triggers in the same transaction, plus a store
  instance name and a compaction boundary. Sweeps keep the newest 4096 rows.
  The existing pending revision is a single counter and the answer log records
  only settlements, so neither can say which card changed at creation, an
  execution report or a prune.
- Move to subc-protocol 0.32.5, subc-client-rs 0.30.1 and (for tests)
  subc-daemon 0.36.14.
- Pin the stream in `consent-watch-pending-v1` without changing existing corpus
  bytes.

## 0.9.1 (ck-cingulate)

- Write `engram-catalog.json` into the module's data folder on every start, so
  Engram backs up `cingulate/store.db` on a fresh machine without a hand step.
  The bytes are deterministic, an identical file is left untouched, replacement
  is atomic and owner-only (0600), and a failed write warns with the path and
  error without stopping the module from serving.

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
