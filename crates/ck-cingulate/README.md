# ck-cingulate

Supervised subc consent module, id `cingulate`, providing `consent/v1` without
requiring an orchestrator, identity, presence or notification provider. Run the
`ck-cingulate` binary under subc. Its management manifest declares precisely ten
consent operations and module-managed concurrency. `health.check` is also handled
by the SDK's control plane and reads only an atomic readiness status.

## Integration and test seams

`Handler::handle_stamped` is the in-process envelope entry point; `Handler` also
implements the SDK's `ModuleHandler`. `Stamp` is supplied exclusively by the
daemon adapter. The production adapter uses the principal from `on_bind` and the
SDK's daemon-attested `scope`: bind session is the JSON tuple
`[owner, ref, epoch]`, and carried agent is `attributes.agent_id`. Unscoped binds
keep both fields empty; carrier requests without an attested agent fail closed.
Answering and listing pending cards require an operator surface: both the
`reserved:callosum` principal and a bind identity `harness` beginning with the
literal `fed-phone:` prefix. When Callosum opens a route for a paired device's
call, it sets that harness to `fed-<class>:<device fingerprint>`, where the class
(`phone`, `sim`, `mac` or `unknown`) is fixed by the operator at pairing and the
device cannot choose it. Only the phone may approve cards. The harness only
narrows Callosum's answer authority; on any other principal it grants nothing. Body identity
and the bind identity's session are never trusted. Console machine stays empty:
no attested console source exists, so `at_console` answers still fail closed.

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
prefrontal-core) check an answered card against the one they sent, and one card,
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

The module uses subc-client-rs 0.27.0, subc-protocol 0.30.0 and the SDK's
subc-transport 0.11.0. The SDK refuses undecodable control requests without
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

## Verification

From the workspace root, run the repository worker guide's format, lint, tests
and independence gates. `cargo test -p ck-cingulate --all-targets --locked` runs
the module-specific tests, including full snapshots of both lifecycle tables
and notifications for no-write refusals, a notifier that reads committed SQL
through an independent connection, and injected-clock await/retry/timer paths.
