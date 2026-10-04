# ck-cingulate

Supervised subc consent module, id `cingulate`, providing `consent/v1` without
requiring an orchestrator, identity, presence or notification provider. Run the
`ck-cingulate` binary under subc. Its management manifest declares precisely ten
consent operations and module-managed concurrency. `health.check` is also handled
by the SDK's control plane and reads only an atomic readiness status.

## Integration and test seams

`Handler::handle_stamped` is the in-process envelope entry point; `Handler` also
implements the SDK's `ModuleHandler`. `Stamp` is supplied exclusively by the
daemon adapter. The production adapter uses the principal from `on_bind`, labels
only `reserved:callosum` an operator, and leaves bind session, carried agent and
console machine fields empty until the SDK's attested sources are specified.
Carrier requests therefore fail closed in production; tests inject full stamps.
No request-body identity or SDK `BindIdentity` field grants authority.

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

The reviewed specification pins subc-client-rs 0.25.2 and subc-protocol 0.28.1.
The SDK source documents per-request task dispatch; a live-daemon test with
attested route fields is still deferred. In-process tests prove the handler's
concurrency and do not claim to prove live routing.

## Verification

From the workspace root, run the repository worker guide's format, lint, tests
and independence gates. `cargo test -p ck-cingulate --all-targets --locked` runs
the module-specific tests, including full snapshots of both lifecycle tables
and notifications for no-write refusals, a notifier that reads committed SQL
through an independent connection, and injected-clock await/retry/timer paths.
