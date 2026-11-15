# Flow-decision phone cards, v1

These vectors serve Basal's three supplied `consent.request` parameter bodies through Cingulate's real handler, then read each pending card and the combined pending page through the phone route. The request bodies are copied byte-for-byte from `crates/cingulate-vectors/tests/inputs/flow-decision-phone-v1`; the generator embeds them unchanged as the `params` value in each handler request. Fresh scratch storage starts at `el_1` and the fixed test clock is 10,000 ms.

All inputs here are invented test data: agents, sessions, cards, devices, ids
and times name no real person, device or deployment. Principals such as
`reserved:callosum` are protocol identifiers, not accounts.

| Files | What they show |
|---|---|
| `decision-reconcile-request.json`, `decision-reconcile-request-reply.json` | Unchanged reconcile params submitted by Basal and the real handler reply. |
| `decision-reconcile-phone-get.json` | Phone `consent.get` reply for the pending reconcile card. |
| `decision-reenable-request.json`, `decision-reenable-request-reply.json` | Unchanged re-enable params submitted by Basal and the real handler reply. |
| `decision-reenable-phone-get.json` | Phone `consent.get` reply for the pending re-enable card. |
| `decision-grant-lost-request.json`, `decision-grant-lost-request-reply.json` | Unchanged lost-grant params submitted by Basal and the real handler reply. |
| `decision-grant-lost-phone-get.json` | Phone `consent.get` reply for the pending lost-grant card. |
| `list-pending.json` | Phone `consent.list_pending` reply containing all three pending cards. |
| `SHA256SUMS` | Hashes of every JSON file and this README. |
| `README.md` | This index and reproduction instructions. |

From the workspace root:

```sh
cargo run -p cingulate-vectors --example generate_flow_decision_phone_vectors --locked -- --check
cargo test -p cingulate-vectors --test flow_decision_phone --locked
```

With `--check`, the generator compares every generated reply, the README, and `SHA256SUMS` byte-for-byte and checks exact directory coverage without writing files. Running without `--check` writes only this corpus. The phone contract test pins the served option ids, labels, effects, defaults, state, title and prompt, as well as the manifest digest.
