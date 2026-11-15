# Package approval card

`request.json` is one complete `consent.request` envelope for a `package` card,
in the exact shape a package-card requester sends. The package (`dark-wake`
version 4), its code hash, facts and options are invented test data; no real
package, agent or person is behind them. Several corpora reuse this envelope as
their package-card input, so its bytes are pinned by hash: a change to the
envelope shows up as a hash mismatch in every test that reads it.

SHA-256: `960d3fd19ee6bcec2cabc22c86063208e6dca5ded4da45bbe4e9b779262be5ed`.

The test `tagged_package_vector_hash_is_pinned_and_card_is_stored_end_to_end`
(`crates/cingulate-vectors/tests/consent.rs`) checks the bytes against that
hash, submits the envelope with the daemon-stamped requester principal
`reserved:prefrontal-core` (the only principal allowed to file package cards),
and checks both the full requester reply and the stored package payload. Package
source and the JSON-encoded manifest remain available through `consent.get`;
card lists omit those two bodies.
