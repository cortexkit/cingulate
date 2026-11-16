# Sealed text v1.2 — served vectors

For Cingulate 0.23.1. All request/reply files are the real handler's
JSON bytes, using a fixed clock and a fresh temporary SQLite store. They cover
filing by AFT, phone and desktop reads, approval and exact retry, decline,
filer-only `answer_inputs`, redacted read-back, idempotent release, and all seven
request/answer refusal cases. `SHA256SUMS` covers every file except itself.

`session-notices.json` and `pointer-notices.json` are the durable session notice
bodies and notification pointers. They contain no public key or sealed bytes;
notices may omit the optional `input: true` hint. The phone/desktop read-back
has `input_supplied: true`; only filer get/await returns `answer_inputs`, and
after release it returns `input_released: true` instead.

The generator uses Rust `hpke` 0.11 with X25519-HKDF-SHA256 / HKDF-SHA256 /
ChaCha20-Poly1305 in Base mode, with empty additional authenticated data (AAD).
The invented recipient keys below are public, **test-only** fixtures, not
production secrets:

- Raw X25519 private key (hex): `005f1745058397e767449339f0e3910c1f945e96456260f688182a904313412c`
- Raw X25519 public key (hex): `d3fbeddb8b14ac9b47cc931184a92e03b08077adbcb9719101e0105af4c64a5c`

The HPKE `info` is the RFC 8785 canonical JSON binding the card, field, round,
and version. Its exact UTF-8 string is:

```text
{"card":"el_1","field":"password","round":0,"v":"ck-sealed-text/v1"}
```

Its bytes in hex are
`7b2263617264223a22656c5f31222c226669656c64223a2270617373776f7264222c22726f756e64223a302c2276223a22636b2d7365616c65642d746578742f7631227d`.

To open the approve sample, base64url-decode its unpadded `enc` and `ciphertext`
from `params.inputs[0]`, then use the private key, the suite above, the exact
`info` bytes and empty AAD. The resulting UTF-8 plaintext is
`invented-sudo-password`. Opening with empty `info` must fail authentication.
The recipient key pair is derived from 32 bytes of decimal 41; sender randomness
is ChaCha20Rng seeded with 32 bytes of decimal 42. Cingulate itself never
decrypts and has no runtime HPKE dependency.

Regenerate locally from the repository root:

```sh
nice -n 10 cargo run --locked -p cingulate-vectors --example generate_sealed_text_vectors
nice -n 10 cargo run --locked -p cingulate-vectors --example generate_sealed_text_vectors -- --check
```

The samples use a daemon-stamped AFT head scope, so subjects and session notices
are attributed to the stamped agent. The v1 and v1.1 corpora are frozen; v1.2
retains v1.1's card-bound HPKE bytes while updating identity and fixed refusal messages.

The wire answer uses existing camelCase `elicitationId`, `choiceId`, and
`expectedRound`. Filed input definitions retain their original snake_case
members. Releases use `consent.release_inputs {elicitation_id}`. The durable
sweep also releases on expiry/withdrawal and within ten minutes of settlement,
including across restart. Exact answer retries still work after release using a
SHA-256 fingerprint stored with the answer; a changed answer conflicts.
