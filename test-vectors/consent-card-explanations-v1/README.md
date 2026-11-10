# Consent card explanations, v1

Corpus tag: `consent-card-explanations-v1`. These compact JSON envelopes are
generated from the Cingulate handler, not hand-written response examples.
They pin the [explanations contract](../../docs/contracts/consent-card-explanations-v1.md)
introduced in ck-cingulate 0.17.0 (`consent-card-explanations/v1`).

The invented **Meadow Digest** version 2 has both author-text objects with
core-supplied labels (at most 64 scalars), text (at most 8,192 UTF-8 bytes),
newlines and a UTF-8 café example. Each `flow-install-*`
and `package-*` group contains:

- `request.json`: the `consent.request` envelope core sends.
- `app-get.json`: the `consent.get` result envelope, with camelCase app keys.
- `app-list-pending.json`: the `consent.list_pending` result envelope. Each
  entry preserves both objects; package scripts and manifests stay omitted.
- `requester-get.json`: the requester `consent.get` result envelope.

Phone and desktop apps share the same app-facing card representation. Read a get sample
with `{"method":"consent.get","params":{"elicitationId":"el_1"}}` and a
list sample with `{"method":"consent.list_pending","params":{}}`.
Each group uses its own scratch store, a stopped clock at 10,000 ms and `el_1`.
No private names, live stores or secrets are used.

`SHA256SUMS` covers every JSON file and this README. The corpus test pins the
checksum manifest itself, checks exact file coverage and regenerates served
replies byte for byte. Apps may pin the manifest and the two app readbacks per
kind independently of requester envelopes.

From the repository root:

```sh
cargo run -p cingulate-vectors --example generate_consent_card_explanations_vectors --locked -- --check
cargo test -p cingulate-vectors --test consent_card_explanations --locked
```

To intentionally change samples, run the generator without `--check`, review
the response bytes and `SHA256SUMS`, then update the manifest hash in the test.
