CREATE TABLE config_plan_audit (
    elicitation_id TEXT NOT NULL,
    answer_seq INTEGER NOT NULL,
    choice TEXT NOT NULL,
    answered_at_ms INTEGER NOT NULL,
    statement_sha256 TEXT,
    statement_length INTEGER,
    unverified_code TEXT,
    reported_at_ms INTEGER,
    PRIMARY KEY (elicitation_id, answer_seq)
);

-- Keep the delivery order and history of old notices while giving a reopened
-- card a separate push/retraction cycle. Existing cards remain in round zero.
CREATE TABLE access_notices_with_round (
    elicitation_id TEXT NOT NULL,
    purpose TEXT NOT NULL,
    body_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    retry_at_ms INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    round INTEGER NOT NULL DEFAULT 0 CHECK (round >= 0),
    PRIMARY KEY (elicitation_id, purpose, round)
);
INSERT INTO access_notices_with_round
    (rowid, elicitation_id, purpose, body_json, status, attempts, retry_at_ms, last_error)
    SELECT rowid, elicitation_id, purpose, body_json, status, attempts, retry_at_ms, last_error
    FROM access_notices ORDER BY rowid;
DROP TABLE access_notices;
ALTER TABLE access_notices_with_round RENAME TO access_notices;
