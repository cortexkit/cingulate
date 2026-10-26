-- Request ids remain reserved after card retention so a signed request can
-- never be presented again as a different card.
CREATE TABLE secret_use_requests (
    request_id TEXT PRIMARY KEY,
    elicitation_id TEXT NOT NULL
);
ALTER TABLE elicitation_records ADD COLUMN secret_use INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cingulate_changes ADD COLUMN secret_use INTEGER NOT NULL DEFAULT 0;
DROP TRIGGER cingulate_change_card_insert;
DROP TRIGGER cingulate_change_card_update;
DROP TRIGGER cingulate_change_card_delete;
CREATE TRIGGER cingulate_change_card_insert AFTER INSERT ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,rich_ui_device,secret_use)
        VALUES (NEW.elicitation_id,NEW.rich_ui_device,NEW.secret_use);
END;
CREATE TRIGGER cingulate_change_card_update AFTER UPDATE ON elicitation_records
WHEN OLD.state IS NOT NEW.state OR OLD.record_json IS NOT NEW.record_json
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,rich_ui_device,secret_use)
        VALUES (NEW.elicitation_id,NEW.rich_ui_device,NEW.secret_use);
END;
CREATE TRIGGER cingulate_change_card_delete AFTER DELETE ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,rich_ui_device,secret_use)
        VALUES (OLD.elicitation_id,OLD.rich_ui_device,OLD.secret_use);
END;
