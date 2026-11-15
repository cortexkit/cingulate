# presence/v1 samples

These bytes are produced by ck-cingulate's route classification and management
handlers, its streaming sink, and cingulate-presence's routing function. They do
not use a consent store. Times are receiver-stamped Unix epoch milliseconds.
The phone and two computer apps merge in `get-merged.json`; `watch-sequence.json`
contains opening, coalesced change and expiry frames. `route.json` includes inline
suppression, stale attention, kinds the watched agent's apps don't draw inline and every fallback location.

All inputs here are invented test data: agents, sessions, cards, devices, ids
and times name no real person, device or deployment. Principals such as
`reserved:callosum` are protocol identifiers, not accounts.

Generate from the repository root:

```
cargo run -p cingulate-vectors --example generate_presence_vectors --locked
cargo run -p cingulate-vectors --example generate_presence_vectors --locked -- --check
```

`SHA256SUMS` covers every file except itself. The test pins the manifest digest;
consumers should pin that digest with the release's git tag.
