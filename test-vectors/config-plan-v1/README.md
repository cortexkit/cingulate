# Config-plan v1 served vectors

Readers first: CKIOS must read these samples and render `configPlan` and
`unverified` before Callosum sends cards. Phones opt in with
`clientFeatures: ["config-plan"]`; older readers never receive this kind.

Generate from the real handler using
`cargo run -p cingulate-vectors --example generate_config_plan_vectors`.
Use `-- --check` to compare the checked-in bytes, including SHA256SUMS.
All module/project names and bytes are invented. The statement deliberately
contains non-UTF-8 and non-JSON bytes, and the signature is fake: Cingulate must
carry both unchanged, not parse or verify them. Callosum is the verifier.

`person-notices-two-rounds.json` pins the four scope-less person notices for Core:
escalated/settled with round omitted (zero), then escalated/settled with round 1.
Core must key intake, push receipts and terminal checks by id, purpose and round.

The samples cover filing, pending list/get, an approval and requester get/await,
unverified reopening, a second answer with sequence 2, decline, withdrawal,
expiry and wrong-filer/missing-statement refusals.
