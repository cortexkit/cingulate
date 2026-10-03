# Consent as its own module (`cingulate`)

Status: draft for review (ALF, 2026-10-03). Reviewers: ENTO, CEREB, CKIOS, the Callosum owner, BASAL, PLEX.

## Why

Consent cards ("elicitations") live in prefrontal-core today, which causes three problems:

1. **Everything depends on core.** Cerebellum, Plexus, Basal and Entorhinal's root approvals can't ask the user anything while core is down, and a replacement orchestrator would have to rebuild the card plane.
2. **"Phone" is hardcoded.** The answering route is the operator's phone (`route.fed_operator`), and delivery is an APNs push. A card should reach the surface the user is on now, and some cards can only be answered at one machine.
3. **Three jobs are tangled.** Core stores the card, chooses how to reach the user, and tells the requesting agent. Only the last needs core.

## Decision

A dedicated module owns the card plane and serves a capability, `consent/v1`. Requesters depend on the capability, never on a module id (the module replaceability rule), the same way modules depend on `project-identity/v1` today. Any implementation that serves the capability can replace it.

| Job | Owner |
|---|---|
| Card lifecycle: request, store, expire, answer, withdraw, outcome feed | consent module |
| Reaching the user: surface choice, presence, push and retract | consent module, reaching surfaces through Callosum (the federation module that connects machines, devices and later VMs) |
| Telling the agent: outcome notice into the requesting head's session | the orchestrator (prefrontal-core today), from the outcome feed |
| Standing grants ("always allow") | each provider (Cerebellum, AFT, Plexus); a card only offers them as options |
| Who an agent or the operator is | Entorhinal; the consent module reads it |

The subc daemon does what it does now: it stamps verified caller identity, so the consent module can trust requesters and answerers. No card state goes into the daemon.

## Multiple machines

Each machine runs its own modules, including its own consent module. A card is never copied or moved between machines:

- **A card lives on the machine where its requester runs.** The provider waiting on the answer is local, the card is short-lived, and only one machine has anything to decide. Replication would add a second authority for a decision that has exactly one.
- **Surfaces reach cards through Callosum's federation.** Alfonso apps connect to every machine Callosum federates, list pending cards across all of them, and send each answer back to the card's own machine. `at_console: <machine_id>` refers to the requester's machine.
- **Mirroring is for durable shared state that every machine needs**, such as Claustrum credentials and Insula quota: one leased writer per item (for example one OAuth refresher) and replicas everywhere else. Cards are the opposite case.
- **Standing grants follow the resource.** A browser grant is per machine because the browser profile is; a grant on a shared service follows wherever that service runs.

## Interface sketch (`consent/v1`)

Carried over from core's ops, which have run in production since migration 194:

- `consent.request`: a requester raises a card. The fields are as today: kind, title, prompt, 2 to 4 options with effects (grant, decline, choose), default, urgency, facts, target, expiry of at most 24 h, late_execution, and the subject agent. The requester's identity comes from the daemon stamp, checked against a per-kind allowlist.
- `consent.await`: a bounded blocking wait (at most 2 minutes per call).
- `consent.get`: available to the owner, and to answering surfaces through a surface projection.
- `consent.answers` and `consent.ack`: the owner's cursor feed of settled cards.
- `consent.withdraw`: the requester settles its own pending card.
- `consent.answer`: only from a route stamped as one of the operator's surfaces.
- `consent.list_pending`: for surfaces.

Invariants that carry over unchanged:
- every card defaults to a decline, and silence never grants;
- the first settlement wins;
- states are pending, then exactly one of answered, expired or withdrawn;
- push payloads are pointers, never card content.

New:
- **The subject is an Entorhinal agent id,** plus an optional session reference that the module treats as opaque. The module resolves names for display from Entorhinal and does not interpret sessions.
- **Requester identity comes only from the daemon's stamp,** resolved through Entorhinal for display ("ALF wants to ..."). An agent id in the request body is never trusted; a forged one is refused, as core does today.
- **Direct callers are accepted.** A process running as the operator outside any route (for example an agent shelling out to `ck`) may raise a card. It has no verified agent identity, so the card names it as "an unverified local process", the same way flow install cards label a local author.
- **Answerability constraint** on each card: `anywhere` (the default), or `at_console: <machine_id>`, meaning answerable only by someone at that machine, unlocked. `computer_use` cards use `at_console`.
- **Outcome subscribers.** The orchestrator subscribes to outcomes for the cards it raised for its heads and delivers notices itself. The consent module never injects into agent sessions.

## Routing by presence

- **Presence** comes from the surfaces:
  - Callosum knows which devices are connected;
  - the desktop app knows it has focus and recent input;
  - Cerebellum knows whether the Mac's screen is locked.
- **Delivery:** a card goes to the surface the user is active on, else to the phone. Answering on one surface retracts it on all of them.
- **`at_console` cards:** these reach only that machine's desktop surface. If the machine is locked or the user is away, the module does not page the phone. It either holds the card until unlock, within its expiry, or refuses at once with `user_not_at_console`, depending on what the requester asked for. A computer-use request then fails fast with a reason the agent can act on, instead of waiting out a long card.

## Dependency direction

```
requesters (Cerebellum, Plexus, Basal, Entorhinal, orchestrator)
        │  consent/v1
        ▼
  consent module ──reads──▶ Entorhinal (agent and operator identity)
        │
        ├──▶ surfaces (Callosum: phone, desktop; presence)
        └──▶ outcome feed ──▶ orchestrator ──▶ notice into the head session
```

The consent module depends on Entorhinal and the surfaces, never on the orchestrator.

## Migration outline

1. Stand up the module with `consent/v1`, the store copied from `elicitation_records` and `elicitation_answers`, and today's requester allowlist (Plexus: consent; Basal: flow_install and flow_decision; core on behalf of heads: consent for browser and computer actions).
2. Requesters switch to the capability. Core's ops then become relays to it, and are deleted in the same cut once callers have moved; there is no long coexistence.
3. Core keeps only two things: raising cards for its heads (the Cerebellum reverse-consent path) and the notice delivery from the outcome feed.
4. Surfaces move to the module's answer and list ops; the phone's operator route becomes one of the surface stamps.
5. **Push leaves core too.** Today core sends APNs itself (`DirectApnsTransport` in `push_sender.rs`, holding the APNs key) and reads only device tokens and sealing keys from Callosum's device registry. Waking a device is a federation and surface job, so the APNs sender moves next to Callosum as a notify-a-surface capability. The consent module, asks and anything else that wakes a device request it, instead of each holding an Apple key.

## Open questions

1. **Card plane in Callosum or a separate module?** The draft leans separate. Callosum is the federation layer: it connects machines and devices but holds no product state. A card store with its own state and audit is a different job.
2. **Who renders typed card kinds?** Card kinds with typed bodies (`flow_install`, `flow_decision`) are rendered into facts by core today, from the hashed manifest. Should the requester send already-rendered facts, or does the consent module own a registry of typed kinds? The trust question decides it: the install card must show what the hashed manifest says, not what Basal claims.
3. **Where presence lives:** inside the consent module, or as its own small capability that other modules (wake routing, do-not-disturb) could also read?
4. **Operator surfaces in Entorhinal:** which devices count as the operator's surfaces, and is that an Entorhinal fact?
5. **Where "always allow" lives.**
   - **One store in the consent module** gives one list to review and revoke, and requesters stop building grant stores. But providers check grants on every tool call, so that check becomes a cross-module call on the hot path: if the consent module is down, every tool refuses. The store would also have to model each provider's grant meaning (Cerebellum's levels, origins and app-signer eligibility; Plexus's per-connection actions; AFT's tool patterns).
   - **Grants stay with providers** behind a common interface (`grants.list`, `grants.revoke`, `grants.would_ask`; Cerebellum already serves the first and last). Enforcement then stays local and fast, and the consent module or the desktop app lists and revokes across all providers through that interface.
   - The draft leans toward the second; this is the operator's call.
6. **Sequencing with the identity move:** Entorhinal refuses agent-initiated identity writes with a typed error until a consent capability is bound, so neither move waits for the other.
7. **Revoking on an offline machine.** A revoke goes from a surface to the owning provider through Callosum. If that machine is off the federation but still running agents, the grant keeps working there until the revoke arrives. Proposal: Callosum queues the revoke and delivers it on reconnect, and the app shows "revoke pending" until then; broad grants carry an expiry; and the riskiest kinds (for example "always allow on all sites") may require the machine to be federated at the moment of use.

## Review round 1: settled (2026-10-03, #consent-module)

The module is named `cingulate` (operator approval), and ALF owns it. These rulings supersede the draft where they differ:

1. **Cingulate renders nothing.** Facts are opaque labelled rows, shown with the requester the daemon stamped ("Prefrontal core asks ...", "basal asks ..."). Typed bodies travel unrendered for the phone and the vectors. The authority that holds the record raises the card: core raises `flow_install` (it holds the hashed manifest and runs the `grants.would_ask` checks), and Basal raises `flow_decision`. This answers open question 2.
2. **A provider mints a grant only from an answer to a card it raised itself.** `grant_lost` becomes a routing card ("Ask <provider> for access again", "Keep the flow disabled" as default, "Uninstall"). "Ask again" calls the provider's `grants.offer {agent_id, flow_id, items}`, which only the flows plane may call. The provider raises its own standing-grant card or answers `offer_not_available {reason}` (Plexus always does: always-allow is an operator action there). Basal resumes only when `grants.would_ask` answers `yes`. The first card's outcome text says what happens next ("Cerebellum will ask you to confirm access to <origin>"), and the provider raises its card before replying.
3. **`consent.request` carries an opaque `binding`,** stored and returned verbatim with the answer, so requesters key their own state on it. The subject agent and the stamped requester are separate record fields.
4. **Identity reads fail open for display, never for authority.** If Entorhinal is down, the card shows the raw agent id. A requester or subject that Entorhinal resolves as retired is refused. Entorhinal holds no operator record and no devices (this answers open question 4); answer authority is Callosum's expose profile, and subc's `origin` field carries device attribution only. Entorhinal never holds a store transaction across `consent.await`.
5. **Error codes:** `consent_not_bound` (no capability configured; permanent) and `consent_unavailable` (bound but down; transient). They need opposite handling, so they never share a code. This settles open question 6.
6. **Presence is its own capability, `presence/v1`** (answers open question 3), fed by:
   - the phone's `presence.report {state, lastInputMs}` on scene transitions and the poll beat, expiring after about 35 s of silence;
   - the desktop app's focus;
   - Cerebellum's `screen_locked`;
   - Callosum's local `sessions_read`.
   Unknown never means present, and the consent module and wake routing both read it.
7. **`at_console` applies to the takeover card only.** Cerebellum already refuses app actions on a locked screen before raising a card, and app cards stay answerable from anywhere. An answer stamped `reserved:callosum` never satisfies `at_console`. The phone lists such cards read-only as "Answer at <Mac>".
8. **Push becomes Callosum's `surface-notify/v1`.** It is a stateless op that seals a pointer payload and submits it through the rendezvous worker, which already holds the APNs key, so core's `DirectApnsTransport` and its key are deleted rather than moved. The rest of the push plan:
   - **Order:** the phone ships Auth-mode opening, the Mac re-enrolls for a notification key, then the sender switches.
   - **Ids:** card ids are never reused across raises, and retractions carry the raise's id.
   - **Machines:** pushes name the sending machine (the Auth key id does).
   - **Corpus:** the commons sealed-push corpus gains an Auth case.
9. **Multiple machines.**
   - **Inbox:** cards are keyed by (machine_id, id); the inbox merges per-machine list calls, with an explicit "machine unreachable" row.
   - **Revoke queue:** held by the app or the acting machine's cingulate, never by Callosum, retrying an idempotent `grants.revoke(grant_id)`.
   - **Lost machine:** stopped by device revocation and roster retirement; broad grants carry an expiry.
   - **"Federated at use":** if ever needed, it means "revocations synced within N minutes". Cerebellum will not check federation on the hot path. This answers open question 7.
10. **Cutover order:** cingulate ops live, the phone build granting `consent.*` ships, then requesters switch. The record shape (`elicitation-records-v1`) is kept, and the common grant shapes (`grants.list`, `grants.revoke`, `grants.would_ask`, `grants.offer`) are pinned in commons with vectors. In Plexus, "always allow" is a policy (`allow | ask_first | block`), not a grant, and `grants.list` labels module-layer rows separately.
11. **Callosum is separate from the card plane** (answers open question 1). Callosum holds no product state; `consent.answer` over Callosum is ledgered exactly once.
12. **Grants stay with providers** (answers open question 5), as above.

**Immediate hardening, ahead of the cutover:** the `sim` expose profile loses `elicitation.answer` and every other mutating op, keeping reads. Until then a same-user process can answer cards as a simulator. Cerebellum holds its standing browser grants, including "always allow on all sites", until that is done. Entorhinal admits no `reserved:callosum` identity mutations until simulators are out of the answering and mutating profiles.

**On-behalf-of: settled by targeted carriers** (SUBC, ENTO). A provider raising a card for the agent that called it (Cerebellum, Plexus, Entorhinal) opens its route to cingulate under that agent's session scope. At `route.open` the daemon checks that the provider is a listed carrier on the scope and that cingulate is among its allowed destinations. Cingulate reads the subject agent from the stamp on its own inbound route; the requester is the carrier. Both fields are daemon-attested, so `consent.request` carries no requester-supplied subject.
- **What the stamp proves:** the provider was entitled to carry that session to cingulate, not that the agent called the provider just now. Carrier entitlement is per session, not per call. The remaining trust is "this provider may act for this agent, toward cingulate only", which the orchestrator grants and the daemon checks. That is bounded, but it is not zero.
- **Destinations are module ids,** not capabilities. Carrier entries name `cingulate`, so discovery binds by `consent/v1`, but replacing the implementation means the orchestrator rewrites the carrier lists it owns.
- **Core's work:** list `reserved:entorhinal`, `reserved:cerebellum` and `reserved:plexus` as carriers on the agent sessions that can call them, with `destinations: ["cingulate"]` only. Cerebellum today raises cards on the reverse lane of core's inbound route; it becomes an outbound carrier like the others.
- **Plexus is a real change, not a rename** (PLEX, measured): it opens its card route to prefrontal-core unscoped under one fixed bind session and puts the subject in the request body; its `reserved:plexus` carrier entry is declared but unused. At cutover it opens the route per card under the held call's scope selector and drops the body subject; a card for an ended or re-epoched scope fails `scope_ended` at `route.open`.
- **Carrier restraint:** a provider opens its scoped route to cingulate only while it holds a live inbound call from that session (CEREB, ENTO). The daemon can't enforce this, so each provider pins it with a test.
- **Simulators can't answer** (CALLO, live 16:37Z): the `sim` expose profile carries only query ops; `elicitation.answer` and the other ten mutating ops reach the `phone` profile only. Entorhinal's future phone-facing creates go to `phone`, never `sim`.
- **Unscoped `direct` callers** have no scope, so their cards name "an unverified local process".
- The daemon keeps no per-call record, so after-the-fact call lookups are not available.
