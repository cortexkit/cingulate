# cingulate-store

Synchronous consent persistence with no dependency on an orchestrator or subc.
`cortexkit-store` owns the connection, WAL configuration and single-writer lease.
The schema is namespaced `cingulate`. Migration 1 creates the card table
(`elicitation_records`) and the settled-card answer log (`elicitation_answers`),
and migration 2 seeds the persistent `el_` id counter. Later migrations in
`migrations/` are numbered in order and each runs once.

## Module integration

- Call `storage_descriptor(hello_ack_descriptor.as_ref(), data_home)`; a supplied
  descriptor is used verbatim. Only absence uses
  `sqlite_store_path(data_home, "ck-cingulate")`, with module id `cingulate`.
- `ConsentStore::open(&descriptor, now_ms)` migrates and sweeps, returning
  `(store, settlements)`. Await retractions for those settlements before serving
  ops. Retry only `Error::is_lease_held()`; other errors are not contention.
  A store ahead of this binary's schema chain is refused.
- Authorize the route stamp and validate params before any sweep or mutation.
  Use the JSON array `[principal, bind_session]` as `RequestIdentity.owner`.
  Populate requester and subject only from that stamp and the resolver. Supply
  `resolved_name` only for a successful name resolution, not a raw-id fallback.
- Call `sweep(now_ms)` on each timer tick and before get, answers, outcomes,
  pending-list, answer, withdraw and every await read. Sweep prunes and returns
  only newly settled records. Request and execution reporting do not run global
  sweeps. A rejected request must not settle unrelated cards.
- `create` consumes an accepted `ConsentRequest`. `created == false` is a
  refresh: wake all waiters for the record, but send no pointer. For a new card,
  await retractions in `settlements` order, then the new delivery.
- `answer` and `withdraw` return a `Mutation` with committed settlement events;
  retries and conflicts produce no new events. `answer` accepts the stamped
  console machine id and checks it only after expiry, state and option checks.
  The module must enforce operator authorization before calling it.
- Send one retraction per returned settlement, using `answered_at_ms`. New
  deliveries use `created_at_ms`. Notify only after these methods return; an
  error means the transaction rolled back, and nothing should be notified.
- `get_owned`, `withdraw` and `report_execution` distinguish an unmatched owner
  (`elicitation_not_owner`) from an acknowledged/tombstoned card
  (`elicitation_not_found`). `owner` remains readable until its row is pruned.
- `answers` is owner-scoped and omits withdrawals. Restrict `outcomes` to
  `reserved:prefrontal` in the module. Both return the global persisted
  high-water, even if no matching rows remain. Outcomes omit binding and retain
  the card's `requester_ref`; they include `single_use` and `late` only when true.
  `late` is captured from the stored detach state in the settlement transaction.
  `ack` is cumulative and owner-scoped, including withdrawals.
- `ElicitationRecord::phone` and `requester_wire` build the wire projections.
  Supply the greatest registered await deadline as `block_until`; projection
  clamps it only when an expiry exists and omits it on terminal cards. Cards
  without expiry store SQL NULL and omit expiry from every projection. Pending
  cards sort by creation time then id, independent of expiry. The module
  owns waiter registration, clock waits, notifications and locking between ops.

All tests use scratch directories under the system temp directory and inspect
the real SQLite rows, including rollback, acknowledgement and retention paths.
