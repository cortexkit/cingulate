CREATE TABLE elicitation_records (
    elicitation_id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    dedup_key TEXT,
    record_json TEXT,
    state TEXT NOT NULL CHECK (state IN ('pending', 'answered', 'expired', 'withdrawn')),
    expires_at_ms INTEGER NOT NULL,
    settled_at_ms INTEGER,
    acknowledged INTEGER NOT NULL DEFAULT 0,
    notice_delivered INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX elicitation_open_dedup ON elicitation_records(owner, dedup_key) WHERE state = 'pending' AND dedup_key IS NOT NULL;
CREATE INDEX elicitation_expiry ON elicitation_records(expires_at_ms) WHERE state = 'pending';
CREATE TABLE elicitation_answers (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    elicitation_id TEXT NOT NULL UNIQUE REFERENCES elicitation_records(elicitation_id) ON DELETE CASCADE,
    owner TEXT NOT NULL,
    settled_at_ms INTEGER NOT NULL
);
CREATE INDEX elicitation_answers_owner ON elicitation_answers(owner, cursor);
CREATE INDEX elicitation_retention ON elicitation_records(settled_at_ms) WHERE settled_at_ms IS NOT NULL;
CREATE INDEX elicitation_unnotified ON elicitation_records(settled_at_ms) WHERE state != 'pending' AND notice_delivered = 0 AND record_json IS NOT NULL;
