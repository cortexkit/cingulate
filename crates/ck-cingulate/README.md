# ck-cingulate

> [!WARNING]
> **Alpha.** ck-cingulate is in early development and is not yet released for general use.
> Breaking changes will happen between versions, including changes to its settings,
> stored data and interfaces. Pin an exact version if you depend on it.

Supervised subc consent module, id `cingulate`, providing `consent/v1` and `presence/v1` without
requiring an orchestrator, identity, presence or notification provider. Run the
`ck-cingulate` binary under subc. Its management manifest declares seventeen
management operations and module-managed concurrency. `health.check` is also handled
by the SDK's control plane and reads only an atomic readiness status.

## Presence

`presence.report` replaces one device's in-memory activity. The route admits the
reporter and sets its kind; only a development-switch `direct` caller names its
own app.
`presence.get` reads fresh devices and the shared summary; `presence.watch`
streams that summary, coalescing changes within 250 ms and waking on the nearest
expiry. These paths never acquire the consent-store lock and work before the
store opens. Core and local operator-standing callers can read; only paired
phone routes and local operator-standing apps can report. The verified
`operator_app` stamp overrides the body's app; development `direct` uses a
validated app code. See the [complete contract](../../docs/contracts/presence-v1.md)
and [generated samples](../cingulate-vectors/presence-v1). The manifest declares
the capability and all three operations; this module has no `role.describe` handler.

## Integration and test seams

`Handler::handle_stamped` is the in-process envelope entry point; `Handler` also
implements the SDK's `ModuleHandler`. `Stamp` is supplied exclusively by the
daemon adapter. The production adapter uses the principal from `on_bind` and the
SDK's daemon-attested `scope`: bind session is the JSON tuple
`[owner, ref, epoch]`, and carried agent is `attributes.agent_id`. Unscoped binds
keep both fields empty; carrier requests without an attested agent fail closed.
Answering and listing pending ordinary cards require an operator surface: a
daemon-verified `operator_app` bind stamp, or both the `reserved:callosum` principal
and a bind identity `harness` beginning with the literal `fed-phone:` prefix.
When Callosum opens a route for a paired device's
call, it sets that harness to `fed-<class>:<device fingerprint>`, where the class
(`phone`, `sim`, `mac` or `unknown`) is fixed by the operator at pairing and the
device cannot choose it. `secret_use`, `manifest_binding`, `config_plan`, and
`flow_install` cards carrying secrets retain their phone-only rules in default
builds; desktop recognition alone is not a signed approval.
The harness only
narrows Callosum's answer authority; on any other principal it grants nothing. Body identity
and the bind identity's session are never trusted. Console machine stays empty:
no attested console source exists, so `at_console` answers still fail closed.

## Development local operator

The `dev-local-operator` Cargo feature is off by default. For a development
installation that needs plain local (`Principal::Direct`) programs to answer
ordinary and vault-approval cards, build with:

```sh
cargo build -p ck-cingulate --release --features dev-local-operator
```

The feature also admits `secret_use`, `manifest_binding` and secret-carrying
`flow_install` cards on the stamped local-operator route, with the phone's
approval feature and envelope shape. Development envelopes must name device
`00000000000000000000000000000000`; top-level `signature` and
`signature_b64url` members are refused, and envelope objects remain closed.
The vault's development mode verifies the envelope except for the signature.
Escalated notices mark these cards with `phone_only: false` in this build. Every
escalated notice carries the authoritative boolean; an absent marker means an
older Cingulate and requires the reader's kind-based fallback. Config plans still
require the phone because the daemon confirms them on the computer itself.
Setting the `DEVELOPMENT_LOCAL_OPERATOR` environment variable has no effect.
Public release builds must omit `dev-local-operator`; build them with
`cargo build -p ck-cingulate --release --no-default-features`.
`ck-cingulate --version` and `health.check` detail report
`dev-local-operator: on` or `dev-local-operator: off` so placement checks can
reject artifacts with development authority.

## Package cards and paging

Prefrontal core may submit `flow_install` and `package` approval requests. A package payload
is a closed object under `package`: string `package`, integer `version`, lowercase
64-hex BLAKE3 `code_hash`, string `script`, and JSON-encoded string `manifest_json`.
It has exactly one grant and one decline option and a decline default.
All consent envelopes are limited to 1,310,720 bytes, including JSON framing;
excess bytes are refused as `elicitation_request_too_large`, never truncated.

`consent.list_pending`, which the phone reads, omits the package `script` and
`manifest_json` bodies; a full card is returned by `consent.get`. `consent.answers`
and `consent.outcomes` keep the bodies: their readers (the card's requester and
prefrontal) check an answered card against the one they sent, and one card,
at most the envelope limit, always fits a page. Core outcomes preserve an opaque
`requester_ref`, include `single_use` only when true, and include `late: true` only
when the card settled after a committed detach.
Each serialized page, including its result envelope, is below 3 MiB. A pending
list with more records returns `continuation: {expires_at_ms, elicitation_id}`;
pass that object unchanged as the next list's `continuation`. Ordering remains
expiry then id. The continuation is a position, not a frozen snapshot: cards
settled during traversal disappear and new cards before it require a fresh list.
Answers and outcomes return an integer `continuation` when more rows remain;
pass it as the next call's `since`. On partial pages `cursor` is the last returned
answer cursor, so acknowledging it cannot discard unread answers. On final pages
the existing global high-water cursor is preserved. No continuation field is
added to an unpaged reply, preserving existing pinned projections.

## Runtime seams

`Options` injects `Clock`, `Resolver`, `Notifier`, an optional `AwaitHook` and the
sweep-timer switch. Production uses wall time, an unavailable resolver and a
no-op notifier. Tests use temporary descriptors and a watch-driven manual clock:
no time passage depends on real sleeping. `stop_timer` stops the recurring timer;
`timer_tick` fires exactly one tick using the same sweep path.

HELLO_ACK storage is deserialized and passed verbatim to `cingulate-store`.
Absence alone selects the shared library's fallback path. Health remains Failing
until open and the restart sweep's committed notifications complete. Lease
contention alone starts an off-hook bounded exponential retry, marked Degraded.
Other open failures and retry exhaustion remain Failing.

The store owns SQLite, WAL, migrations and its lifetime lease. The handler owns
authorization ordering, projections, waiter deadlines and post-commit delivery.
Store locks are released before hooks, clock waits and notifier calls. Waiter
registration is RAII-managed (including cancellation); watch-channel revisions
prevent settlement/refresh wakes from being lost between a read and a wait.
Pointer delivery is awaited in order, including retraction before replacement,
and notification errors cannot reverse a committed state or fail an operation.

The module uses subc-client-rs 0.40.3, subc-protocol 0.37.1 and the SDK's
subc-transport 0.18.0 to receive the daemon's operator-app recognition stamp and
register supervised launches over the child-bound registration channel. The stamp
identifies a recognized desktop operator-app connection; it is not approval.
Bind identities now contain harness and session, not a project-root claim.
The SDK refuses undecodable control requests without
terminating the serve loop. Cingulate does not handle flows or manage agent-run
scopes and declares neither `flow-scopes/v1` nor `agent-run-scopes/v1`.
`tests/live_daemon.rs` runs the real daemon router and the SDK-served module in
one process over authenticated loopback TCP. It checks catalog registration,
requester refusal, owner `scope.sync` followed by a listed carrier's scoped open,
scope-owned cards and epoch isolation, and refusal of direct operator answers
even with a caller-supplied phone harness. Scope records use the protocol's
non-exhaustive constructors and setters. The daemon's supervisor launch-nonce
records are seeded for the in-process owner and carriers; the tests do not prove
supervised spawning or Callosum's paired-phone provenance. The positive phone
case remains covered by the bind-adapter unit tests, without impersonating a
paired phone in the daemon test.

The fixture uses `cortexkit-test-support` scratch directories and checks that its
connection file names the live hosting test process. Connection files, project
roots and storage are explicitly scratch-local; no connection discovery, child
process spawning or process-global XDG changes are needed. Every operation has a
bounded deadline and the fixture cancels and awaits its serving tasks on teardown.
The separate in-process handler tests continue to prove handler concurrency.

## Engram backup catalog

Engram backs up a module only when its data folder holds `engram-catalog.json`.
The module writes its own catalog from the store descriptor on every start, so
the file lands beside `store.db` (the parent of the descriptor's SQLite path)
and a fresh machine needs no hand step before its first backup. The catalog
names one portable `whole-db` entry: Engram snapshots the live database through
the SQLite backup API, which already covers the `-wal` and `-shm` sidecars and
the writer lease, so those are not separate entries.

The bytes are deterministic; an identical file is left untouched so its mtime
does not churn on every start. A different or missing file is replaced
atomically through a same-folder owner-only (0600) temp file and a rename. A
failed write is a warning naming the path and the error: a module that refused
to serve over a backup descriptor would be worse than a missed backup.
`tests/engram.rs` covers the created bytes and mode, the second start, a stale
file and an unwritable location.

## Verification

From the workspace root, run the gates in [CONTRIBUTING.md](../../CONTRIBUTING.md)
(`./scripts/gate.sh`). `cargo test -p ck-cingulate --all-targets --locked` runs
the module-specific tests, including full snapshots of both lifecycle tables
and notifications for no-write refusals, a notifier that reads committed SQL
through an independent connection, and injected-clock await/retry/timer paths.
