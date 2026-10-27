# Status (handover)

Owner: SUBC (from ALF). Cingulate is the consent module: it serves `consent/v1`,
stores consent cards and records their outcome. Design and review rulings:
[design.md](design.md). The [danger option group](contracts/danger-option-group.md)
contract is written ahead of implementation. Cingulate serves the aggregator
side of [permissions/v1](contracts/permissions-v1.md); grant-holding providers
serve the capability itself.

Version 0.13.1 accepts [forwarded v2 vault statements](contracts/consent-requester-v2.md#forwarded-secret-statements-v2-0131)
on `secret_use` and flow-install cards: the v1 statement plus `asking_member_id`,
naming the paired computer that asked. v1 is unchanged. Tests live in
`crates/cingulate-vectors/tests/forwarded_secrets.rs`; vectors in
`secret-use-approval-v2` and `flow-install-secrets-v2`.

Version 0.13.0 adds [flow-install secret approvals](contracts/consent-requester-v2.md#flow-install-secret-approvals-0130)
under `flow-install-secrets/v1`: core files paired `flow_identity` and `secrets`
inside the otherwise opaque install body. Closed canonical vault statements
must match that identity and offer only `version`. Supported phone routes alone
see and answer these cards, using the `secret-use-approval` client feature.
An allow binds every request/digest exactly once to a version-duration envelope
with zero grant expiry and the answering phone's fingerprint. The earliest
statement deadline bounds even a no-expiry filing; an earlier filed expiry wins,
and timeout always denies. No migration is needed: existing JSON records,
answer bindings and retained secret-card visibility flags suffice. Generated
vectors and focused tests live in `flow-install-secrets-v1` and
`crates/cingulate-vectors/tests/flow_install_secrets.rs`.

Version 0.12.0 introduced [vault `secret_use` approvals](contracts/consent-requester-v2.md#vault-secret-use-requester-0120).
Scoped AFT/Cerebellum routes file closed canonical statements; feature-enabled
phones answer with the exact vault envelope. Card records bind its SHA-256,
duration and per-item decisions. Late answers after statement expiry are refused.

## What v1 does

### Operations

The module `cingulate` (binary `ck-cingulate`, `crates/ck-cingulate`) declares
`consent/v1` and fifteen management operations (`OPERATIONS` and `manifest()`,
`crates/ck-cingulate/src/lib.rs:36-71`):

| Operation | Kind | Who may call (`dispatch`, `lib.rs:585-594`) |
|---|---|---|
| `consent.request` | mutate | An allowlisted requester for its card kind (`Stamp::allowed_kind`, `lib.rs:104-113`; checked in `identity`, `lib.rs:495-497`). |
| `consent.get` | query | The owner (snake_case requester projection), or an operator surface (camelCase surface projection by `elicitationId`). |
| `consent.await` | query | The owner. Bounded wait, at most 120 s (`lib.rs:626`). |
| `consent.answers`, `consent.ack` | query, mutate | The owner's cursor feed of settled cards. |
| `consent.report_execution` | mutate | The owner, after a granted card was acted on. |
| `consent.withdraw` | mutate | The owner settles its own pending card. |
| `consent.revoke` | mutate | The owner revokes a granted `rich_ui_approval`. Only `reserved:occipital` may request this kind. |
| `consent.watch_pending` | query/watch | Phone operator routes, filtered by feature and device for `rich_ui_approval` and, for `secret_use`, only `reserved:callosum` routes whose harness is `fed-phone:` plus 32 lowercase hex and whose client advertises `secret-use-approval`. |
| `consent.answer` | mutate | Operator surfaces only (`operator`, `lib.rs:279-288`). |
| `consent.list_pending` | query | Operator surfaces only. |
| `consent.outcomes` | query | `reserved:prefrontal-core` only; preserves `requester_ref`, includes `single_use` and `late` only when true. Readers use `consent-outcomes-late/v1` to interpret absent `late` (`lib.rs:588-592`). |

Identity comes only from the daemon's route stamp (`Stamp`, `lib.rs:73-123`);
no request field names a requester or subject. The owner of a card is the pair
`[principal, bind session]` (`Stamp::owner`, `lib.rs:101-103`). Carrier
requesters (`reserved:cerebellum`, `reserved:plexus`, `reserved:entorhinal`) must
carry an agent on the stamp, or they are refused (`lib.rs:498-504`).

### Package cards and bounded feeds

`reserved:prefrontal-core` may raise `package` cards as well as `flow_install`.
Package payloads are typed and closed; the tagged prefrontal vector is copied and
SHA-256 checked in `test-vectors/consent-package-card-v1/`. The consent envelope
limit is 1,310,720 bytes with `elicitation_request_too_large` on overflow.
The phone's pending list omits package script and manifest bodies; answers and
outcomes keep them, because their readers check the answered card against what
they sent. All three page below 3 MiB, with continuations documented in
`crates/ck-cingulate/README.md`.
A full card remains readable through `consent.get`; existing non-package phone
projections and pinned corpus bytes are unchanged.

### Store

`crates/cingulate-store` holds cards in SQLite through `cortexkit-store`
(connection, WAL, single-writer lease). Migration 1 is prefrontal migration 194
unchanged; migration 2 seeds a persistent `el_` id counter, so card ids are never
reused (`crates/cingulate-store/migrations/`). Settled cards are retained for 24
hours (`RETENTION_MS`, `crates/cingulate-store/src/lib.rs:19`) and then read as
tombstones until pruned. The store's own contract is in
`crates/cingulate-store/README.md`.

### Lifecycle

- A card is `pending`, then exactly one of `answered`, `expired` or `withdrawn`;
  the first settlement wins.
- Every card defaults to a decline with `on_expiry: deny`, so silence never grants
  (`ConsentRequest::validate`, `crates/cingulate-decode/src/lib.rs:262-272`).
- Expiry is applied by a sweep on a timer, on open (downtime expiry), and before
  every read or answer.
- A `dedup_key` refreshes a matching pending card instead of filing a new one; a
  content change withdraws the old card and files the new one atomically.
- Each new card and each settlement sends a pointer (`{elicitationId, kind,
  eventAtMs, machineId?}`, never card content) through the `Notifier`, only after
  the commit (`Handler::pointer`, `lib.rs:472-482`).
- `at_console` cards are answerable only from a stamp carrying that machine id;
  otherwise `user_not_at_console`.

## Which tests prove each part

| Part | Tests |
|---|---|
| Request decoding and the refusal order | `crates/cingulate-decode/tests/decoding.rs`; `crates/cingulate-decode/tests/corpus.rs` against `test-vectors/elicitation-request-v1/` |
| Store: migrations, lease, ids, first-settlement-wins, expiry, dedup, feeds, ack, retention, rollback | `crates/cingulate-store/tests/lifecycle.rs` |
| Module: manifest, envelope, admission per op, allowlist, console gates, await, timer, notifier ordering, lease retry | `crates/ck-cingulate/tests/module.rs` (for example `owner_principal_and_operator_refusals_write_nothing`, `allowlist_table_identity_and_order`, `console_machine_request_and_answer_gates`, `notifier_is_awaited_after_commit_and_errors_do_not_fail_ops`) |
| Real daemon transport: registration/catalog, unlisted requester refusal, scoped carrier ownership and epoch isolation, direct answer refusal | `crates/ck-cingulate/tests/live_daemon.rs`; daemon and SDK module serve in process over authenticated TCP, with scratch connection files and stores and seeded supervisor nonce records |
| Exact current SDK/protocol release pins and lockfile resolutions | `crates/ck-cingulate/tests/dependencies.rs::current_subc_releases_are_exactly_pinned_and_locked` |
| Wire compatibility with core's shapes | `crates/cingulate-vectors/tests/records.rs`, `requester.rs`, `consent.rs`, against `test-vectors/elicitation-records-v1/`, `elicitation-requester-v1/`, `consent-records-v1/` |
| Corpus integrity | `crates/cingulate-vectors/tests/pins.rs` |
| Danger option group and permissions/v1 contracts (vectors only) | `crates/cingulate-vectors/tests/contracts.rs`; `cargo run -p cingulate-vectors --example generate_vectors --locked -- --check` |

The gates are in `.cortexkit/WORKER.md`. All tests use scratch stores under the
system temp directory.

## Placement and what is left

- **Production placement.** `ck-cingulate` runs under the production daemon
  (`cingulate` in `subc.jsonc`) and owns the live cards; prefrontal-core's
  `elicitation.*` operations are deleted and the phone reads `consent.*`. The
  bullets below record what is still left after placement.
- **Attested stamp fields.** The production adapter now reads the SDK's daemon
  `RouteBindRequest.scope`: bind session is `[owner, ref, epoch]` serialized as
  JSON, and carried agent is `attributes.agent_id`. Scoped Cerebellum, Plexus
  and Entorhinal requests can therefore carry their subjects in production.
  Unscoped routes keep the previous empty identity and fail closed for carriers.
  No console machine source exists yet: `at_console` answers remain fail-closed.
  Operator answers require `reserved:callosum` **and** a bind identity harness
  with the literal `fed-phone:` prefix; other paired device classes cannot answer.
  Live-daemon tests now exercise scope sync, carrier admission, stamped bind
  sessions, subject propagation and cross-epoch owner isolation. Direct routes,
  including a forged phone harness, cannot answer. No actual Callosum or paired
  phone is present in this fixture: the positive `reserved:callosum` plus
  `fed-phone:` case remains covered by existing bind-adapter unit tests, not by
  a fabricated phone identity. Supervisor nonce records are seeded for the
  in-process scope owner and carriers, so supervised spawning remains untested.
- **Config.** The manifest's config schema is an empty object. Production
  `Options` use wall time, an unavailable resolver (no Entorhinal names; cards
  show raw agent ids) and a no-op notifier (no pushes until Callosum's
  `surface-notify/v1` exists) (`Options::default`, `lib.rs:192-205`).
- **Push sender switch.** prefrontal-core still pushes card notifications. The
  switch to cingulate's notifier follows the consent push runbook: the Mac's
  notification key moves to Claustrum, the phone ships Auth-mode opening, then the
  sender switches and Callosum admits `reserved:cingulate` on `push_submit`.
- **Not built:** `presence/v1` (cingulate serves it; tagged samples come before
  the phone builds) and holding `at_console` cards until unlock.

## Who consumes which operations

| Consumer | Operations | Where the code or vectors show it |
|---|---|---|
| prefrontal-core | Requester of `flow_install` and `package` cards; the only reader of `consent.outcomes`, from which it delivers notices to its heads; the client whose `elicitation.*` relays cut over | `allowed_kind` (`lib.rs:109`); `consent.outcomes` admission (`lib.rs:588-592`); `test-vectors/elicitation-requester-v1/README.md` (core's dispatch is the source of the requester shapes) |
| Callosum and the phone | `consent.list_pending`, surface `consent.get`, `consent.answer` | `operator_surface` requires `reserved:callosum` and the literal `fed-phone:` harness prefix (`Stamp::from_bind`); `test-vectors/elicitation-records-v1/` and `consent-records-v1/` pin the phone projections |
| Plexus | Requester of `consent` cards as a carrier, then owner ops | `allowed_kind` (`lib.rs:106`), carrier check (`lib.rs:498-504`); `test-vectors/elicitation-requester-v1/README.md` names `reserved:plexus` as the requester |
| Basal | Requester of `flow_decision` cards, then owner ops | `allowed_kind` (`lib.rs:110`) |
| Cerebellum, Entorhinal | Requesters of `consent` cards as carriers | `allowed_kind` (`lib.rs:106`) |
| Local `direct` callers | Requester of `consent` cards, labelled "an unverified local process" | `allowed_kind` (`lib.rs:106`), label (`lib.rs:535-536`) |

## Known gaps

From SUBC. An operation admitted on the local operator's `direct` route proves
only that the caller runs as the operator's user. Any local process connects as
`direct`, an agent's shell included. Admitting a card answer there would be a
self-approval door: an agent could answer its own head's card. This is the same
open gap as §5.8 and §5.10 of the elevation design (`docs/designs/operator-elevation.md`
in the private `cortexkit/subconscious-design` repository). It
closes with the runtime cell (separate runtime user accounts) or a program check
on the caller. It is not a v1 blocker, and it must not be read as a guarantee.

- **Card answers.** In this code `consent.answer` and `consent.list_pending` are
  not admitted on `direct`: both require `operator_surface` (`dispatch`,
  `crates/ck-cingulate/src/lib.rs:586`; `operator`, `lib.rs:279-288`), which only
  `reserved:callosum` with a `fed-phone:` bind harness sets (`Stamp::from_bind`). `owner_principal_and_operator_refusals_write_nothing`
  (`crates/ck-cingulate/tests/module.rs:788-807`) proves a `direct` answer is
  refused with `elicitation_operator_required`. The gap applies to any change
  that admits `direct` answers. `direct` may raise cards (`lib.rs:106`); those
  name "an unverified local process" and approve nothing by themselves.
- **`permissions.revoke`.** The [permissions/v1](contracts/permissions-v1.md)
  contract admits revoke from `direct`, both on providers and on cingulate. The
  same caveat applies: any local process can revoke. The worst case is a local
  process revoking a grant, which only narrows authority. The aggregator's
  admission is in `Handler::permissions` (`crates/ck-cingulate/src/permissions.rs`).
  Its `direct` list admission has the same lack of proof that a person acted.
