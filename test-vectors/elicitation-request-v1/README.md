# Consent request vectors v1

These are raw `elicitation/create` params bodies decoded by
`ConsentRequest::decode` / `ConsentRequest::validate` in
`crates/prefrontal-core-store/src/elicitation.rs`, not JSON-RPC envelopes.
Requester ownership comes from the transport, not these bodies.

Core treats `scope` as an opaque, provider-defined object and requires a present
scope or a nonblank `args_digest`. Scope contents in these vectors are
illustrative, not Cerebellum's schema; Cerebellum owns its scope shapes. The
`target.label` values are illustrative too. These vectors pin core's decoder
verdicts, not provider scope formats.

Consumers copy the files byte for byte and
verify `SHA256SUMS` before using them. Do not reformat JSON or hand-mirror the
validation rules. Pin the SHA-256 of `SHA256SUMS` in the consuming repository;
the manifest covers every other file, including this README and expected
verdicts. The manifest cannot include its own hash. From this directory run
`shasum -a 256 -c SHA256SUMS`.

## Valid cards

- `browser-site-grant`: origins and tier scope, normal urgency.
- `app-grant-once`: app bundle scope, without allow_always or requestedSchema.
- `app-grant-always`: app bundle scope with allow_always. Both app cards omit
  duration: the option ids express once versus persistent permission.
- `takeover`: machine takeover scope, high urgency.
- `import-login`, `capture-login`: illustrative login scopes, high urgency.
- `plexus-args-digest`: digest instead of scope, normal urgency.

These high-urgency operations keep the requesting tool call waiting until the
user answers the consent card.

All other cards include requestedSchema. Its decision enum deliberately uses a
different order from the options: equality is order-independent. The answer
schema is validated but never serialized into the stored request.

## Refusals

Each refused body is a minimal edit of `valid/app-grant-once.json`, paired with
`<name>.expected.json`. Codes must match exactly; message_contains pins a stable
part of the decoder diagnostic rather than the complete serde wording.

- `unknown-field`: `elicitation_invalid_request`, diagnostic contains ``unknown field `unknown` ``.
- `missing-urgency`: `elicitation_invalid_request`, diagnostic contains ``missing field `urgency` ``.
- `bad-urgency`: `elicitation_invalid_request`, diagnostic contains `invalid urgency or late_execution`.
- `missing-scope-and-digest`: `elicitation_invalid_request`, diagnostic contains `provide args_digest or scope`.
- `blank-digest`: `elicitation_invalid_request`, diagnostic contains `provide args_digest or scope`.
- `non-decline-default`: `elicitation_fail_closed_required`, diagnostic contains `decline default and on_expiry deny`.
- `on-expiry-not-deny`: `elicitation_fail_closed_required`, diagnostic contains `decline default and on_expiry deny`.
- `duplicate-option-id`: `elicitation_invalid_request`, diagnostic contains `option ids must be unique and nonempty`.
- `empty-option-label`: `elicitation_invalid_request`, diagnostic contains `labels must be nonempty`.
- `empty-option-id`: `elicitation_invalid_request`, diagnostic contains `option ids must be unique and nonempty`.
- `default-not-option`: `elicitation_invalid_request`, diagnostic contains `default must name an option`.
- `expiry-zero`: `elicitation_invalid_request`, diagnostic contains `expires_in_ms must be positive and at most 24 hours`.
- `expiry-over-24h`: `elicitation_invalid_request`, diagnostic contains `expires_in_ms must be positive and at most 24 hours`.
- `empty-target-kind`: `elicitation_invalid_request`, diagnostic contains `target kind and label must be nonempty`.
- `empty-target-label`: `elicitation_invalid_request`, diagnostic contains `target kind and label must be nonempty`.
- `unknown-kind`: `elicitation_unknown_kind`, diagnostic contains `only consent is supported`.
- `schema-enum-mismatch`: `elicitation_invalid_request`, diagnostic contains `requestedSchema`.
- `schema-enum-not-strings`: `elicitation_invalid_request`, diagnostic contains `requestedSchema`.
- `schema-enum-missing`: `elicitation_invalid_request`, diagnostic contains `requestedSchema`.
- `bad-late-execution`: `elicitation_invalid_request`, diagnostic contains `invalid urgency or late_execution`.
- `empty-title`: `elicitation_invalid_request`, diagnostic contains `title, prompt and nonempty options are required`.
- `empty-prompt`: `elicitation_invalid_request`, diagnostic contains `title, prompt and nonempty options are required`.
- `empty-options`: `elicitation_invalid_request`, diagnostic contains `title, prompt and nonempty options are required`.
- `nested-unknown-field`: `elicitation_invalid_request`, diagnostic contains ``unknown field `unknown` ``.

The core-store integration suite discovers every body and expectation and checks
manifest coverage and byte hashes. Change verdicts deliberately, with consumer
coordination, rather than silently updating pinned vectors.
