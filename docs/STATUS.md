# Status (handover)

Owner: SUBC (from ALF). Cingulate is the consent module: it serves `consent/v1`,
stores consent cards and records their outcome. Design and review rulings:
[design.md](design.md). Contracts written ahead of implementation:
[danger option group](contracts/danger-option-group.md) and
[permissions/v1](contracts/permissions-v1.md).

## What v1 does

### Operations

The module `cingulate` (binary `ck-cingulate`, `crates/ck-cingulate`) declares
`consent/v1` and ten management operations (`OPERATIONS` and `manifest()`,
`crates/ck-cingulate/src/lib.rs:36-71`):

| Operation | Kind | Who may call (`dispatch`, `lib.rs:585-594`) |
|---|---|---|
| `consent.request` | mutate | An allowlisted requester for its card kind (`Stamp::allowed_kind`, `lib.rs:104-113`; checked in `identity`, `lib.rs:495-497`). |
| `consent.get` | query | The owner (snake_case requester projection), or an operator surface (camelCase surface projection by `elicitationId`). |
| `consent.await` | query | The owner. Bounded wait, at most 120 s (`lib.rs:626`). |
| `consent.answers`, `consent.ack` | query, mutate | The owner's cursor feed of settled cards. |
| `consent.report_execution` | mutate | The owner, after a granted card was acted on. |
| `consent.withdraw` | mutate | The owner settles its own pending card. |
| `consent.answer` | mutate | Operator surfaces only (`operator`, `lib.rs:279-288`). |
| `consent.list_pending` | query | Operator surfaces only. |
| `consent.outcomes` | query | `reserved:prefrontal-core` only (`lib.rs:588-592`). |

Identity comes only from the daemon's route stamp (`Stamp`, `lib.rs:73-123`);
no request field names a requester or subject. The owner of a card is the pair
`[principal, bind session]` (`Stamp::owner`, `lib.rs:101-103`). Carrier
requesters (`reserved:cerebellum`, `reserved:plexus`, `reserved:entorhinal`) must
carry an agent on the stamp, or they are refused (`lib.rs:498-504`).

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
| Wire compatibility with core's shapes | `crates/cingulate-vectors/tests/records.rs`, `requester.rs`, `consent.rs`, against `test-vectors/elicitation-records-v1/`, `elicitation-requester-v1/`, `consent-records-v1/` |
| Corpus integrity | `crates/cingulate-vectors/tests/pins.rs` |
| Danger option group and permissions/v1 contracts (vectors only) | `crates/cingulate-vectors/tests/contracts.rs`; `cargo run -p cingulate-vectors --example generate_vectors --locked -- --check` |

The gates are in `.cortexkit/WORKER.md`. All tests use scratch stores under the
system temp directory.

## Left before first placement

- **Daemon card.** No subc registration for `ck-cingulate` exists yet; the module
  has only been run in process. A live-daemon test with attested route fields is
  deferred (`crates/ck-cingulate/README.md`).
- **Attested stamp fields.** The production adapter fills only the principal. Bind
  session, carried agent and console machine id stay empty until the SDK exposes
  their attested sources, so carrier requests (Cerebellum, Plexus, Entorhinal)
  and `at_console` answers fail closed in production
  (`crates/ck-cingulate/README.md`, "Integration and test seams").
- **Config.** The manifest's config schema is an empty object. Production
  `Options` use wall time, an unavailable resolver (no Entorhinal names; cards
  show raw agent ids) and a no-op notifier (no pushes until Callosum's
  `surface-notify/v1` exists) (`Options::default`, `lib.rs:192-205`).
- **Core's client cutover.** prefrontal-core still owns the live card plane. Core
  turning its `elicitation.*` operations into relays to `consent/v1`, then
  deleting them, is a separate core campaign (design.md, "Migration outline").
- **Not built:** `presence/v1` routing, holding `at_console` cards until unlock,
  and the two contracts above.

## Who consumes which operations

| Consumer | Operations | Where the code or vectors show it |
|---|---|---|
| prefrontal-core | Requester of `flow_install` cards; the only reader of `consent.outcomes`, from which it delivers notices to its heads; the client whose `elicitation.*` relays cut over | `allowed_kind` (`lib.rs:109`); `consent.outcomes` admission (`lib.rs:588-592`); `test-vectors/elicitation-requester-v1/README.md` (core's dispatch is the source of the requester shapes) |
| Callosum and the phone | `consent.list_pending`, surface `consent.get`, `consent.answer` | `operator_surface` is set only for `reserved:callosum` (`Stamp::from_principal`, `lib.rs:85-96`); `test-vectors/elicitation-records-v1/` and `consent-records-v1/` pin the phone projections |
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
  `reserved:callosum` sets (`lib.rs:92`). `owner_principal_and_operator_refusals_write_nothing`
  (`crates/ck-cingulate/tests/module.rs:788-807`) proves a `direct` answer is
  refused with `elicitation_operator_required`. The gap applies to any change
  that admits `direct` answers. `direct` may raise cards (`lib.rs:106`); those
  name "an unverified local process" and approve nothing by themselves.
- **`permissions.revoke`.** The [permissions/v1](contracts/permissions-v1.md)
  contract admits revoke from `direct`, both on providers and on cingulate. The
  same caveat applies: any local process can revoke. The worst case is a local
  process revoking a grant, which only narrows authority. Not yet implemented, so
  there is no admission code path to cite.
