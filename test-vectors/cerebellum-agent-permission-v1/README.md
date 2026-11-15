# Cerebellum agent-permission phone cards, v1

These vectors send four `consent.request` parameter bodies, in the shape Cerebellum files for agent permissions, through Cingulate's real handler. The three head cards use Cerebellum's head scope; the worker card resolves its subject through the live head returned by the worker `scope.describe` fixture. Each request reply and phone `consent.get` reply is recorded, followed by one phone `consent.list_pending` reply containing all four cards. The requests are preserved byte-for-byte under `requests/`; the fixed test clock is 10,000 ms and fresh scratch storage assigns IDs `el_1` through `el_4`.

All inputs here are invented test data: agents, sessions, cards, devices, ids
and times name no real person, device or deployment. Principals such as
`reserved:callosum` are protocol identifiers, not accounts.

Both phone methods declare `clientFeatures: ["option-group"]`, so the danger options remain visible. The phone vectors pin each card's elicitation ID, pending state, subject and requester name.

| Files | What they show |
|---|---|
| `requests/head-app-control.json`, `head-app-control-request-reply.json` | Unchanged head app-control params and the real handler reply. |
| `head-app-control-phone-get.json` | Phone `consent.get` reply for the head app-control card. |
| `requests/head-takeover.json`, `head-takeover-request-reply.json` | Unchanged head takeover params and the real handler reply. |
| `head-takeover-phone-get.json` | Phone `consent.get` reply for the head takeover card. |
| `requests/head-website-public-in-call.json`, `head-website-public-in-call-request-reply.json` | Unchanged head website params and the real handler reply. |
| `head-website-public-in-call-phone-get.json` | Phone `consent.get` reply for the head website card. |
| `requests/worker-app-control.json`, `worker-app-control-request-reply.json` | Unchanged worker app-control params and the real handler reply. |
| `worker-app-control-phone-get.json` | Phone `consent.get` reply for the worker card with its head subject. |
| `list-pending.json` | Phone `consent.list_pending` reply containing all four pending cards. |
| `requests/SHA256SUMS` | The checksums of the four request bodies, fixed before any reply was generated. |
| `SHA256SUMS` | Hashes of every other file in this corpus, including the request checksums. |
| `README.md` | This index and reproduction instructions. |

From the workspace root:

```sh
cargo test -p cingulate-vectors --test cerebellum_agent_permission --locked
```

The regeneration test calls the real handler with the byte-pinned request bodies, compares every generated reply and request copy byte-for-byte, rebuilds the corpus checksum manifest, and checks the phone projections. The manifest coverage test also verifies every file under this directory.
