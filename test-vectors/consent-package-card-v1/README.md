# Package approval card

`request.json` is the exact `consent.request` envelope the producer of package
cards (prefrontal-core) sends, captured from its production code path. It is
copied byte for byte from the `prefrontal` repository's Git tag
`consent-package-card-v1` (commit `8b3d59531`), path
`test-vectors/consent-package-card-v1/request.json`, so a change on the producer
side shows up here as a hash mismatch rather than as a refused card in
production. To refresh it, copy the file from a new producer tag and update the
hash below.

SHA-256: `960d3fd19ee6bcec2cabc22c86063208e6dca5ded4da45bbe4e9b779262be5ed`.

The producer test
`tagged_package_vector_hash_is_pinned_and_card_is_stored_end_to_end` checks the
bytes against that hash, submits the envelope with the daemon-stamped requester
principal `reserved:prefrontal-core` (the only allowed package requester), and
checks both the full requester reply and the stored package payload. Package
source and the JSON-encoded manifest remain available through `consent.get`;
card lists omit those two bodies.
