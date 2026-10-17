-- SQLite cannot remove NOT NULL in place. Rebuild the parent and its answer
-- log together so foreign-key cascades cannot discard existing settlements.
CREATE TABLE elicitation_records_rebuilt (
    elicitation_id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    dedup_key TEXT,
    record_json TEXT,
    state TEXT NOT NULL CHECK (state IN ('pending', 'answered', 'expired', 'withdrawn')),
    expires_at_ms INTEGER,
    settled_at_ms INTEGER,
    acknowledged INTEGER NOT NULL DEFAULT 0,
    notice_delivered INTEGER NOT NULL DEFAULT 0
);
INSERT INTO elicitation_records_rebuilt SELECT * FROM elicitation_records;
CREATE TEMP TABLE cingulate_answers_backup AS SELECT * FROM elicitation_answers;
CREATE TEMP TABLE cingulate_answers_sequence AS
    SELECT seq FROM sqlite_sequence WHERE name = 'elicitation_answers';
DROP TABLE elicitation_answers;
DROP TABLE elicitation_records;
ALTER TABLE elicitation_records_rebuilt RENAME TO elicitation_records;
CREATE TABLE elicitation_answers (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    elicitation_id TEXT NOT NULL UNIQUE REFERENCES elicitation_records(elicitation_id) ON DELETE CASCADE,
    owner TEXT NOT NULL,
    settled_at_ms INTEGER NOT NULL
);
INSERT INTO elicitation_answers SELECT * FROM cingulate_answers_backup;
-- Keep the high-water mark even if retention deleted the last answer rows.
UPDATE sqlite_sequence SET seq = MAX(seq, COALESCE((SELECT seq FROM cingulate_answers_sequence), 0))
    WHERE name = 'elicitation_answers';
INSERT INTO sqlite_sequence (name, seq)
    SELECT 'elicitation_answers', seq FROM cingulate_answers_sequence
    WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'elicitation_answers');
DROP TABLE cingulate_answers_backup;
DROP TABLE cingulate_answers_sequence;
CREATE UNIQUE INDEX elicitation_open_dedup ON elicitation_records(owner, dedup_key) WHERE state = 'pending' AND dedup_key IS NOT NULL;
CREATE INDEX elicitation_expiry ON elicitation_records(expires_at_ms) WHERE state = 'pending';
CREATE INDEX elicitation_answers_owner ON elicitation_answers(owner, cursor);
CREATE INDEX elicitation_retention ON elicitation_records(settled_at_ms) WHERE settled_at_ms IS NOT NULL;
CREATE INDEX elicitation_unnotified ON elicitation_records(settled_at_ms) WHERE state != 'pending' AND notice_delivered = 0 AND record_json IS NOT NULL;
CREATE TRIGGER cingulate_pending_insert AFTER INSERT ON elicitation_records
WHEN NEW.state = 'pending'
BEGIN
    UPDATE cingulate_pending_revision SET value = value + 1 WHERE singleton = 1;
END;
CREATE TRIGGER cingulate_pending_update AFTER UPDATE ON elicitation_records
WHEN (OLD.state = 'pending' OR NEW.state = 'pending')
    AND (OLD.state <> NEW.state OR OLD.record_json <> NEW.record_json)
BEGIN
    UPDATE cingulate_pending_revision SET value = value + 1 WHERE singleton = 1;
END;
CREATE TRIGGER cingulate_pending_delete AFTER DELETE ON elicitation_records
WHEN OLD.state = 'pending'
BEGIN
    UPDATE cingulate_pending_revision SET value = value + 1 WHERE singleton = 1;
END;
