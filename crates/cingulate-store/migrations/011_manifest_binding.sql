-- Preserve kind visibility even after card retention removes the JSON payload.
CREATE TABLE manifest_binding_requests (
    statement_sha256 TEXT PRIMARY KEY,
    elicitation_id TEXT NOT NULL
);
ALTER TABLE elicitation_records ADD COLUMN manifest_binding INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cingulate_changes ADD COLUMN manifest_binding INTEGER NOT NULL DEFAULT 0;
DROP TRIGGER cingulate_change_card_insert;
DROP TRIGGER cingulate_change_card_update;
DROP TRIGGER cingulate_change_card_delete;
CREATE TRIGGER cingulate_change_card_insert AFTER INSERT ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding)
        VALUES (NEW.elicitation_id,NEW.secret_use,NEW.manifest_binding);
END;
CREATE TRIGGER cingulate_change_card_update AFTER UPDATE ON elicitation_records
WHEN OLD.state IS NOT NEW.state OR OLD.record_json IS NOT NEW.record_json
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding)
        VALUES (NEW.elicitation_id,NEW.secret_use,NEW.manifest_binding);
END;
CREATE TRIGGER cingulate_change_card_delete AFTER DELETE ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding)
        VALUES (OLD.elicitation_id,OLD.secret_use,OLD.manifest_binding);
END;
