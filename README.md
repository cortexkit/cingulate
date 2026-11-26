# cingulate

> [!WARNING]
> **Alpha.** Cingulate is in early development and is not yet released for general use.
> Breaking changes will happen between versions, including changes to its settings,
> stored data and interfaces. Pin an exact version if you depend on it.

Cingulate is the permission-card module of the [CortexKit](https://github.com/cortexkit) stack.
When an agent or a module wants to do something that needs a person's approval, it files a
consent card with Cingulate. Cingulate stores the card, shows it on the surfaces where the user
can answer (the paired phone, or the desktop app), records the first answer, and hands the
outcome back to the requester.

The name comes from the anterior cingulate cortex, the part of the brain that notices a conflict
before an action and holds the action until the conflict is resolved. This module does the same
thing: it records the decision while the requester holds the action.

## How it fits

```
requesters (agents' tools, modules, the orchestrator)
        │  consent/v1, over the subc daemon
        ▼
  ck-cingulate ──▶ answering surfaces: the paired phone (through Callosum) and the desktop app
        │
        └──▶ outcomes ──▶ the requester acts, or not
```

- **The subc daemon** supervises `ck-cingulate` as the module `cingulate` and carries every call.
  It stamps each connection with the caller's verified identity, and Cingulate reads requester,
  subject and answering surface only from that stamp, never from a request body.
- **Requesters** file cards through the `consent/v1` capability, not this module's id, so any
  implementation that serves `consent/v1` can replace it. Each card kind has a fixed list of
  principals allowed to file it.
- **The apps** answer. The phone reaches Cingulate through Callosum, CortexKit's federation
  module; the desktop app connects locally and is recognised by the daemon as the operator app.
  Cards bound to a signed statement (secret use, flow installs carrying secrets, manifest
  bindings, configuration plans) can only be answered from the phone.
- **Presence.** Apps report short-lived activity through `presence/v1`, so the stack can route
  asks to the surface the user is on. Presence stays in memory and never grants consent.

## What it does

- **Cards.** A requester files a card: a title, a prompt, options, an optional expiry, and
  labelled facts. Option counts depend on the kind; informational macOS cards have none.
  Actionable cards default to a decline: silence, expiry, withdrawal or an error never grants.
  Wait timeouts and delivery errors can leave a card pending. The first answer wins, except
  that failed config-plan verification reopens the card for a new answer round.
- **Outcomes.** Requesters wait for a bounded time or read a cursor feed of settled cards. An
  orchestrator that filed a card for one of its agents turns the outcome into a notice for that
  agent itself. Cingulate also sends structured `session.notice` lifecycle deliveries, which
  can contain option labels, tool paths, notes and subject labels. Phone push notifications,
  unlike those session notices, carry pointers rather than card content.
- **Tool-permission asks.** When a worker agent's tool call needs permission, the ask goes
  first to its head (the agent that started the worker and directs it) and reaches the user
  only if the head escalates it.
- **Permissions.** Standing grants ("always allow") stay with the provider that enforces them.
  Cingulate lists and revokes them across providers through `permissions/v1`.

Cingulate renders nothing, does not retrieve vault secret values, and never decrypts sealed inputs. See
[SECURITY.md](SECURITY.md) for exactly what it defends and what it does not.
Releasing sealed inputs is logical deletion from live rows: SQLite WAL files, backups and
snapshots may retain the ciphertext bytes until overwritten.

## Repository layout

| Path | What it is |
|---|---|
| [`crates/ck-cingulate`](crates/ck-cingulate) | The module binary `ck-cingulate`: authorization, projections, the subc integration. |
| [`crates/cingulate-store`](crates/cingulate-store) | The durable card store (SQLite) and its lifecycle rules. |
| [`crates/cingulate-decode`](crates/cingulate-decode) | Closed wire decoding and fail-closed validation of requests. |
| [`crates/cingulate-vectors`](crates/cingulate-vectors) | Test-only: checks the handler against the wire vectors and generates them. |
| [`docs/contracts`](docs/contracts) | The wire contracts for requesters, the phone and desktop, presence and permissions. |
| [`test-vectors`](test-vectors) | Versioned, checksummed wire samples other components pin. All of their data is invented. |
| [`docs/STATUS.md`](docs/STATUS.md) | What is implemented, which tests prove it, and the known gaps. |
| [`docs/CHANGELOG.md`](docs/CHANGELOG.md) | Changes per crate version. |

[`docs/design.md`](docs/design.md) and [`docs/designs/`](docs/designs) keep the reasoning behind
the design; the contracts are authoritative.

## Build and test

You need only the stable Rust toolchain; every dependency comes from crates.io.

```sh
cargo build --workspace --locked
cargo test --workspace --all-targets --locked
./scripts/gate.sh        # everything CI checks: format, clippy, tests, vectors, independence
```

The tests run the module in process, and the live-daemon tests start a real subc daemon in
process over loopback, all with scratch stores under a temporary directory. Nothing touches a
real CortexKit installation.

`dev-local-operator` is a development-only Cargo feature that lets any local program answer
ordinary cards. It is off by default and release builds never contain it; see
[crates/ck-cingulate/README.md](crates/ck-cingulate/README.md#development-local-operator) and
[SECURITY.md](SECURITY.md).

## Install

Cingulate runs as a module under the CortexKit supervisor. Release binaries for macOS (arm64),
Linux (x64, arm64) and Windows (x64, arm64) are published on this repository's
[releases](https://github.com/cortexkit/cingulate/releases) page, each zip with a SHA-256
sidecar, and are installed with:

```sh
ck setup cingulate
```

Version 0.23.1 requires **subc-core 0.21.6 or newer**: it is the first core release carrying
the daemon line with both `operator-app/v1` and `registration_channel` (daemon 0.45.3).
Public installation must wait for a public subc-core 0.21.6 or newer; the newest public core
at this release's preparation is `subc-core-v0.21.3`. `ck setup` checks the release manifest
and refuses an older core rather than partially installing an incompatible component.

Each release's notes and `release-manifest.json` name the minimum core. `ck-cingulate` is not
published to crates.io, so `cargo install` does not install it.

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) for the design-first process and the gate. Report
vulnerabilities privately as [SECURITY.md](SECURITY.md) describes.

## License

MIT; see [LICENSE](LICENSE).
