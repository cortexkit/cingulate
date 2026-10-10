-- Keep polling revisions in the same transaction as pending-card changes.
CREATE TABLE cingulate_pending_revision (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    value INTEGER NOT NULL CHECK (typeof(value) = 'integer' AND value >= 0)
);
INSERT INTO cingulate_pending_revision (singleton, value) VALUES (1, 0);

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
