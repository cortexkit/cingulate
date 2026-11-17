# Desktop development cards v1

These invented samples were served by ck-cingulate 0.25.4 built with the
`dev-local-operator` feature. The desktop app's development route is a
daemon-stamped `Principal::Direct` local-operator route. Release builds omit
that feature; ordinary operator-app routes cannot read these cards. Config-plan
cards remain phone-only. The applicable client features are
`secret-use-approval` for secret-use and secret-bearing flow-install cards and
`manifest-binding-approval` for manifest-binding cards.

Each JSON file is an object containing `method`, `params` and the handler's
`reply` (`result` or transport `error`). The reply values are the actual handler
projections, formatted as JSON for readability. The `*-list.json` files list one
pending card; `*-list-reply.json` contains the handler's untouched response bytes
for the same list. `*-allow.json` and `*-decline.json` contain independent answers to
fresh copies of that card. `*-signature-refusal.json` adds a top-level
`signature_b64url` to the allow request and records its refusal. The
`*-missing-feature-list.json` files omit client features on the development
route; `*-operator-list.json` uses a plain operator-app stamp with the local-operator
flag disabled, as in a build without the development feature.
All those lists omit the sensitive card. `config-plan-local-list.json` shows that
a local development route cannot see a config-plan card, even when opted in.

Allow envelopes are RFC 8785 canonical JSON encoded as unpadded base64url, with
the fixed development fingerprint `00000000000000000000000000000000`.
Secret-use uses `once` and an expiry 600000 ms after issue; a flow uses `version`
and expiry 0. The vault's other expiry rules are hour +3600000 ms, session
+86400000 ms, and permanent 0. The manifest binding uses its own approval
envelope. These are examples, not signatures or vault grants. Silence, errors
and timeouts never grant approval.

`SHA256SUMS` covers each file other than itself. Regenerate or check with:

```sh
cargo run --locked -p cingulate-vectors --features ck-cingulate/dev-local-operator --example generate_desktop_dev_cards_vectors
cargo run --locked -p cingulate-vectors --features ck-cingulate/dev-local-operator --example generate_desktop_dev_cards_vectors -- --check
```
