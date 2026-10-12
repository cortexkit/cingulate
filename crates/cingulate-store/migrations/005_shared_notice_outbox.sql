-- The notice outbox now also carries late-answer notices for generic consent
-- cards, whose ids live in elicitation_records rather than access_asks. Rebuild
-- it without the foreign key to access_asks, keeping every row, its delivery
-- state and its delivery order (rowid).
CREATE TABLE access_notices_rebuilt (
    elicitation_id TEXT NOT NULL,
    purpose TEXT NOT NULL,
    body_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    retry_at_ms INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    PRIMARY KEY (elicitation_id, purpose)
);
INSERT INTO access_notices_rebuilt
    (elicitation_id, purpose, body_json, status, attempts, retry_at_ms, last_error)
    SELECT elicitation_id, purpose, body_json, status, attempts, retry_at_ms, last_error
    FROM access_notices ORDER BY rowid;
DROP TABLE access_notices;
ALTER TABLE access_notices_rebuilt RENAME TO access_notices;
