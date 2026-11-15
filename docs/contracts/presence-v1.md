# presence/v1 — Cingulate 0.15.0

Presence is a separate, memory-only management capability. It is a routing hint,
not consent or authority. Restarting Cingulate empties it (`where: unknown`). No
report, focus id or summary is written to the consent store, outbox or backups.

## `presence.report` (mutate)

Parameters: `{device_kind?, app?, state, focus?, shows_inline?, ttl_ms}`.

- `state`: exactly `active` (device in use, app frontmost or used in the last
  35 seconds), `background` (device in use, app neither), `idle` (120 seconds
  without input), or `locked`.
- `focus`: when present, 1–16 distinct agent ids, each 1–256 printable non-space
  ASCII bytes, following subc-protocol's opaque-field rule.
- `shows_inline`: when present, 0–8 distinct item-kind strings matching
  `^[a-z][a-z0-9_]{0,31}$`. Known kinds are `ask`, `card`, `mention`; other valid
  kinds are retained. The phone sends `[]` today.
- Focus and inline kinds are validated in every state but retained only while
  active. Invalid fields return `invalid_params` with `{field: "state"}`,
  `{field: "focus"}`, `{field: "shows_inline"}` or `{field: "ttl_ms"}` in the
  error message, using Cingulate's existing code/message error envelope.
- `ttl_ms`: an integer from 1 to 120,000 inclusive. Cingulate stamps
  `reported_at_ms` and `expires_at_ms = now + ttl_ms` with its own clock; an
  absolute time in the request has no effect.

The daemon route decides who may report and the device kind. Only a local
`direct` caller under the development switch names its own app:

| Route | Device id | Kind |
| --- | --- | --- |
| `reserved:callosum`, `fed-phone:<fingerprint>` (32 lowercase hex digits) | `phone:<fingerprint>` | `phone` |
| Verified local operator app (`operator_app.app` stamp) | `computer:<stamped app>` | `computer` |
| Local `direct` with the `dev-local-operator` Cargo feature | `computer:<body app>` | `computer` |

`device_kind` is accepted and ignored for every reporter; the kind comes from
the route. Phone routes also ignore body `app`. Verified operator stamps win
over body `app`. A local app code must match `^[a-z][a-z0-9-]{0,31}$`; absent or
invalid codes give `invalid_params` naming `app`. All other reporters, including
every Callosum `fed-sim:`, `fed-computer:` and other non-phone route, get
`presence_device_not_allowed`. An unverified harness name never establishes app
identity. The manifest requests `operator-app/v1` so the daemon forwards the
verified stamp.

A report replaces the sole previous report for its device id. Reply:
`{device_id, expires_at_ms}`. Expiry is exclusive: a report is fresh only when
`expires_at_ms > now`.

Reporters must re-report before `ttl_ms` passes while their state holds. A
reporter sends `active` with an agent in `focus` only while that agent's chat
is drawn and its window is frontmost or the app had input in the last 35
seconds; otherwise it sends `background` with no focus. The idle threshold is
120 seconds without input. A reporter that can't read idle time never sends
`idle`: it reports `active` and `background` as usual, and `locked` when the
screen locks.

## `presence.get` (query)

Parameters: `{}`. Readers are `reserved:prefrontal-core` and local
operator-standing callers (including verified apps and development `direct`).
Everyone else, including the phone, gets `presence_read_not_allowed`.

Reply:

```
{
  devices: [{device_id, device_kind, state, focus?, shows_inline?, reported_at_ms, expires_at_ms}],
  where: "phone" | "computer" | "away" | "unknown",
  watching: [{agent_id, shows_inline: [kind], seen_at_ms}]
}
```

Only fresh reports are returned, sorted by `device_id`. Empty device focus and
inline lists are omitted. `watching` has one entry per agent in active focus,
sorted by `agent_id`. Its inline kinds are the sorted, de-duplicated union across
those reports, and `seen_at_ms` is their newest `reported_at_ms`.

Location precedence: any active phone → `phone`; otherwise any active or
background computer → `computer`; otherwise any fresh report → `away`;
otherwise `unknown`.

## `presence.watch` (query, watch delivery)

Parameters: `{}`; same readers as get. A stream can also be refused
`presence_unavailable` while Cingulate stops, or `watch_capacity` when every
watch slot, shared with consent watches, is taken. Uses Cingulate's existing
stream transport, emitting raw `{where, watching}` items. The opening item is
the current summary. Later items appear only when location, agent membership or
inline kinds change. A timestamp-only refresh does not emit an item; the newest
timestamps are included on the next meaningful frame or get.

Changes within 250 milliseconds are coalesced. Expiry participates in the same
coalescing window. Each stream schedules only the soonest report expiry and a
pending coalescing deadline, never a periodic poll of reports. Unsubscribe,
route close and module shutdown end the stream. Presence uses only a small map
mutex, never the consent-store lock, and never awaits while holding it.

## Shared routing crate

`cortexkit-presence` (crates.io, published from the shared commons library) is
pure Rust with serde types and no I/O, async runtime or store dependency.
Cingulate uses `summarize(reports, now_ms)` for both get and watch. Core
depends on the same crate by version and calls
`route(summary, kind, agent_id, now_ms) -> Deliver`:

1. `none` if that agent's watching entry lists the kind and
   `now_ms - seen_at_ms <= 60_000`;
2. otherwise `desktop` for `where: computer`;
3. otherwise `phone` (including away and unknown).

All times are Unix epoch milliseconds. This release does not add a `deliver`
field to card watches. Generated, checksum-pinned contract samples live in
[`crates/cingulate-vectors/presence-v1`](../../crates/cingulate-vectors/presence-v1).
