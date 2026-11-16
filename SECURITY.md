# Security policy

## Reporting a vulnerability

Report vulnerabilities privately through GitHub's private vulnerability reporting: open the
repository's **Security** tab and choose **Report a vulnerability**
(<https://github.com/cortexkit/cingulate/security/advisories/new>). Do not open a public issue,
pull request or discussion for a suspected vulnerability.

Include what you found, the commit or release it affects (`ck-cingulate --version` prints both
the version and whether the development feature below is compiled in), and the steps or a
proof of concept that reproduce it. A local reproduction with a scratch store and the test
vectors in this repository is enough; please do not test against someone else's machine or
devices.

## What Cingulate is trusted to do

Cingulate is the permission-card module of the CortexKit stack. It runs on the user's own
machine as a module supervised by the subc daemon, stores consent cards, shows them to the
user's answering surfaces, and records the first answer. Its guarantees:

- **Silence never grants.** Every actionable card has a decline default and `on_expiry: deny`.
  Expiry and withdrawal never grant. An unreachable surface, a wait timeout or an error can
  leave a card pending; they are not approval and do not necessarily settle it. Informational
  macOS cards have no answer options. A settled card keeps its first outcome, except that a
  config-plan verification failure can reopen the card for a new answer round.
- **Identity comes from the daemon.** The requester, the subject agent and the answering surface
  are read only from the route stamp the subc daemon attaches to each connection. A requester,
  subject or device named in a request body is never trusted. Each card kind has a fixed list
  of principals allowed to file it.
- **Only operator surfaces answer.** An ordinary card is answered from the paired phone (a
  `reserved:callosum` route whose harness, the device-class string Callosum sets on the
  route and the daemon attests, starts with `fed-phone:`) or from a
  desktop app the daemon recognises as the operator app (its `operator_app` stamp). A
  requester can never answer its own card through its requester route, and a simulator or
  other paired device class cannot answer.
  Tool-permission asks also allow a verified core relay for the stamped head, within the
  recorded head policy; that is not authority for arbitrary requester answers.
- **Signed-statement cards need the phone.** These kinds are visible to, and answerable from,
  only the paired phone advertising the matching client feature. The answer must carry the
  approval bound to the exact signed statement the card was filed with for a grant:
  - `secret_use` (a request to use a vault secret) and `flow_install` cards that carry secrets:
    the phone's answer is the vault's signed approval envelope, and Cingulate records the
    SHA-256 of that envelope and refuses late answers after the statement expires;
  - `manifest_binding`: the phone decodes the vault's statement itself and refuses a card
    whose served fields disagree with it;
  - `config_plan`: the phone answers with a statement and signature; Callosum verifies them,
    and a failed verification reopens the card rather than granting.

  The desktop operator-app stamp and the development feature below never satisfy these kinds.
- **Phone pushes carry pointers, not content.** The phone notification names a card id and an
  event, not card text or signatures. Structured `session.notice` deliveries are different:
  they can carry content such as option labels, tool paths, notes and subject labels. Treat
  them as private card content, not as content-free notifications.
- **Permission audit is bounded.** Unprivileged answer routes never write permission audit records.
  Authorized rejected attempts retain fixed reason codes and, for malformed bodies,
  byte lengths rather than raw params. Each ask retains at most 32 attempts with
  a dropped count; upgrades normalize historical records atomically as well.

## What Cingulate never does

- **It never decrypts sealed inputs.** A `sealed_text` card carries a one-time public key; the
  answering surface encrypts the typed text to that key, and Cingulate stores the ciphertext
  and hands it only to the card's filer. Cingulate has no decryption key and no runtime dependency on
  HPKE (the hybrid public-key encryption scheme the inputs are sealed with).
  Releasing sealed inputs is **logical deletion** from live SQLite rows, not secure erasure:
  WAL files, backups and snapshots may retain ciphertext bytes until overwritten.
- **It never retrieves vault secret values.** Secret-use and secret-bearing cards carry the
  vault's signed statements and approvals; Cingulate does not verify the vault's signatures,
  call the vault, or receive the secret value. The vault checks the approval itself.
  Arbitrary requester-authored card text can still contain sensitive information.
- **It never holds standing grants.** "Always allow" lives with each provider that enforces
  it. Cingulate lists and revokes those grants through `permissions/v1`, and revoking only
  narrows authority.
- **It never executes an answer.** Requesters read outcomes and act on them. Cingulate also
  sends structured lifecycle notices to daemon-stamped sessions; it does not execute their text.

## What it does not defend against

Stated plainly, so nobody relies on a property Cingulate does not have.

- **Programs running as the same user.** The subc daemon stamps a plain local program as
  `direct`. A `direct` caller may file an ordinary card (labelled "an unverified local
  process") and may list and revoke grants through `permissions/v1`. Such a program can also
  read the daemon's connection file, the store under the user's data directory and the module's
  memory, as it can for any process of that user. Separate user accounts are the boundary;
  Cingulate does not add one.
- **The `dev-local-operator` build feature.** This Cargo feature is off by default and must not
  be enabled in release builds. When it is compiled in, any `direct` program of the user,
  including an agent's own shell, may answer ordinary cards without the phone, including cards
  it filed itself. It does not open the signed-statement kinds above. `ck-cingulate --version`
  and `health.check` report `dev-local-operator: on` or `off`, so an installation can refuse a
  development build; the `DEVELOPMENT_LOCAL_OPERATOR` environment variable has no effect.
- **A compromised daemon, phone or desktop app.** Cingulate trusts the daemon's stamps and the
  answers that arrive on operator routes. Someone who controls the daemon, the paired phone or
  the recognised desktop app can answer cards.
- **Console-bound cards.** A card marked `at_console` may only be answered by someone at that
  machine. Nothing yet attests to Cingulate that an answer came from the machine's own
  console, so such answers are refused rather than trusted.
- **What a requester does with an answer.** A grant answer is a record; the requester decides
  whether and how to act on it, and reports execution itself.
