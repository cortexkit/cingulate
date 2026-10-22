# Rich UI approval v1

Cards and replies captured from Cingulate 0.11.0, with a fixed scratch-store clock
and the phone fingerprint from the copied Callosum device-class harness vectors.
The phone renders its own verified manifest, not requester-provided card words.

`pending`, `granted`, `declined`, `withdrawn`, and `revoked` are complete phone
`consent.get` cards. Grants and declines have wire state `answered`, distinguished
by `answeredChoiceId`. `feature-with` is the complete served card;
`feature-without` is the phone get refusal. Answer files contain the exact params
and result/error envelope. The non-requester revoke file contains params and its
refusal. Fingerprints in these public test fixtures are synthetic, not raw keys.

Regenerate with `cargo run -p cingulate-vectors --example generate_rich_ui_approval_vectors --locked`.
Append `-- --check` to check served bytes and SHA256SUMS without writing.
`rich_ui_samples_are_served_bytes_and_hashes_match` checks served JSON and every
SHA256SUMS entry, including this README.
