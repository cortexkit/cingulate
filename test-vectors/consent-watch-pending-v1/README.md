# Consent pending-card watch, v1

Served bytes from the real `consent.watch_pending` handler, with a scratch
store, a fixed clock and the store's change-log instance pinned to
`0123456789abcdef0123456789abcdef`. Every `*-request.json` is the request
envelope; every other file is one stream item exactly as emitted, or a refusal
encoded as `{"code","message"}`. Items carry no trailing newline.

`live-*` follow one watch opened without a cursor (`live-request.json`) on the
phone's route:

- `live-01-reread.json`: the first item with no cursor is `{"type":"reread"}`;
  the phone then reads `consent.list_pending` once.
- `live-02-created.json`: an unverified local process (`direct`) creates an
  app-access card `el_1` at 10000 ms.
- `live-03-answered.json`: the phone answers `el_1` with `allow_once` at
  11000 ms; the card carries `answeredChoiceId`.
- `live-04-created.json`, `live-05-withdrawn.json`: the same requester creates
  `el_2` and withdraws it.
- `live-06-pruned.json`, `live-07-pruned.json`: a day after both settled, the
  next sweep prunes `el_1` and `el_2` from the phone's view; each item is
  `{"type":"removed"}` with its `elicitation_id`.

`changed` items carry `card`, the phone projection byte for byte as the
phone's `consent.get` returns it. Every `changed` and `removed` item carries
`cursor`, an object `{"instance","version"}` the phone echoes back unchanged;
`version` increases with every item.

`replay-request.json` resumes from the cursor of `live-03-answered.json`.
`replay-01.json` and `replay-02.json` are what follows that cursor: one item
per card at its latest change, in version order, so both cards appear once,
as removed.

`stale-cursor-request.json` sends a cursor older than the retained change log
(retention set to one row, three cards created, then a sweep);
`stale-cursor-reread.json` is the reread it gets.

`oversized-reread.json`: a package card whose projection exceeds the
192 KiB item limit is sent as a reread instead.

`refused-reply.json`: a Callosum route from a paired Mac (`fed-mac:`) is
refused exactly as `consent.list_pending` refuses it.

Regenerate with
`cargo run -p cingulate-vectors --example generate_watch_pending_vectors --locked`;
append `-- --check` to verify every byte and SHA256SUMS without writing.
