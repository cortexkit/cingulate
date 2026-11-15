# Sealed text v1 — served vectors

Tagged-ready for Cingulate 0.23.0. All request/reply files are the real handler's
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
ChaCha20-Poly1305 in Base mode, empty info and additional authenticated data
(AAD). It seals the invented UTF-8
plaintext `invented-sudo-password`. The recipient key pair is derived from 32
bytes of decimal 41; sender randomness is ChaCha20Rng seeded with 32 bytes of
decimal 42. These are public fixture seeds, not usable production secrets.
Cingulate itself never decrypts and has no runtime HPKE dependency.

Regenerate locally from the repository root:

```sh
nice -n 10 cargo run --locked -p cingulate-vectors --example generate_sealed_text_vectors
nice -n 10 cargo run --locked -p cingulate-vectors --example generate_sealed_text_vectors -- --check
```

The wire answer uses existing camelCase `elicitationId`, `choiceId`, and
`expectedRound`. Filed input definitions retain their original snake_case
members. Releases use `consent.release_inputs {elicitation_id}`. The durable
sweep also releases on expiry/withdrawal and within ten minutes of settlement,
including across restart. Exact answer retries still work after release via an
answer-row-only SHA-256 fingerprint; a changed answer conflicts.
