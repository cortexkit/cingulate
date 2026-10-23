# Device-class harness strings

`harness.jsonl` holds the bind-identity harness Callosum sets when it opens a
route for a paired device's call, one row per device class, all for one fixed
test key that belongs to no real device. Callosum generates the file from its
production stamp function and fails its own tests if the two differ.

Cingulate treats a route as the operator's phone only when the principal is
`reserved:callosum` and this harness starts with the literal `fed-phone:`. The
test `callosum_harness_vectors_mark_only_the_phone_as_operator` binds each row
and requires exactly the `phone` row to be the operator surface.

Source: the `callosum` repository, tag `device-class-harness-vectors-v2.0`
(commit `8fe42ea4`), path `test-vectors/device-class-harness/harness.jsonl`.
The computer row's class is `computer`; older Callosum builds stamped `mac`,
which Cingulate still accepts.
SHA-256: `da9f5e4d09bcae2dc70045bf4e629b215a1407d8e13eb26275639568488d892c`.
To refresh it, copy the file from a newer Callosum commit and update the hash
here and in the test.
