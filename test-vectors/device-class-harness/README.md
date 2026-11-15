# Device-class harness strings

`harness.jsonl` holds the bind-identity harness string Callosum sets when it
opens a route for a paired device's call, one row per device class. The strings
have the production format, but every row is built from one fixed, invented test
key that belongs to no real device.

Each harness is the device class's prefix (`fed-phone:`, `fed-sim:`,
`fed-computer:` and so on) followed by a 32-character lowercase hex device
identifier. Cingulate treats a route as the operator's phone only
when the principal is `reserved:callosum` and this harness starts with the
literal `fed-phone:`. The test
`callosum_harness_vectors_mark_only_the_phone_as_operator`
(`crates/ck-cingulate/src/lib.rs`) binds each row and requires exactly the
`phone` row to be the operator surface.

The computer row's class is `computer`; older Callosum builds stamped `mac`,
which Cingulate still accepts.
SHA-256: `da9f5e4d09bcae2dc70045bf4e629b215a1407d8e13eb26275639568488d892c`.
If the harness format changes, replace the file and update the hash here and in
the test together.
