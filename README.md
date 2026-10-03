# cingulate

The CortexKit consent module. It asks the user a question when an agent or module wants to do something that needs a person's approval, and it records the answer.

The name comes from the anterior cingulate cortex, the part of the brain that notices a conflict before an action and holds the action until the conflict is resolved. This module does the same thing: it holds an action until someone says yes.

## What it does

- **Cards.** A requester raises a consent card (a title, a prompt, two to four options, an expiry). The module stores it, routes it to a surface where the user can answer, and records the first answer, or expiry or withdrawal, as the outcome. Every card defaults to a decline, so silence never grants anything.
- **Outcomes.** Requesters wait for a bounded time or read a cursor feed of settled cards. An orchestrator that raised a card for one of its agents turns the outcome into a notice for that agent itself; this module never writes into agent sessions.
- **Presence.** Cards go to the surface the user is using now (desktop app or phone). A card that can only be answered at one machine's console is never sent to a phone.

## What it does not do

- It holds no standing grants ("always allow"). Each provider keeps and enforces its own, behind a common `grants` interface.
- It renders nothing. A card's facts are labelled rows that the requester supplies, shown together with the requester identity the subc daemon attests.
- It does not depend on any orchestrator. It reads agent identity from Entorhinal and reaches devices through Callosum.

## Interface

The module serves the `consent/v1` capability. Requesters bind to the capability, not to this module's id, so any implementation that serves `consent/v1` can replace it.

The design and its review rulings are in [docs/design.md](docs/design.md).

## Status

Design settled; implementation not started.
