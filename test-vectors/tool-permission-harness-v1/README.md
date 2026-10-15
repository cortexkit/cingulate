# Tool-permission harness v1 vectors

These are hand-written examples of reply or refusal bodies from a server other than Cingulate: the Thalamus harness or AFT. They cover codes whose contract-table emitters include `thalamus-harness` or `aft` but not `cingulate`: `access_dismissed`, `access_declined_or_dismissed`, `access_unrecognized`, `access_stale`, `access_no_answer`, `access_not_placed`, `access_use_cingulate`, `access_call_unknown`, `access_conflict`, and `access_queue_full`. `access_not_placed` arises both at request time and from `consent.await`, so it has two files: `access_not_placed.json` (the refusal) and `access_not_placed.outcome.json` (the settled outcome). Cingulate never emits these codes. They pin the outcome and refusal wire shapes listed in [`tool-permission-outcomes-v1`](../../docs/contracts/tool-permission-outcomes-v1.md).

`SHA256SUMS` covers every file in this directory except itself.
