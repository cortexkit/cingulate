# Device-class harness strings

`harness.jsonl` holds the bind-identity harness Callosum sets when it opens a
route for a paired device's call, one row per device class, all for one fixed
test key that belongs to no real device. Callosum generates the file from its
production stamp function and fails its own tests if the two differ.

Cingulate treats a route as the operator's phone only when the principal is
`reserved:callosum` and this harness starts with the literal `fed-phone:`. The
test `callosum_harness_vectors_mark_only_the_phone_as_operator` binds each row
and requires exactly the `phone` row to be the operator surface.

Source: the `callosum` repository, commit `537b10e`, path
`test-vectors/device-class-harness/harness.jsonl`.
SHA-256: `093493871164662fa6ee02c8240a4e048496d6f36870d1db330fa6725d3f6b35`.
To refresh it, copy the file from a newer Callosum commit and update the hash
here and in the test.
