CREATE TABLE broker_action_requests (
    statement_sha256 TEXT PRIMARY KEY,
    elicitation_id TEXT NOT NULL
);

-- Keep feature visibility after payload pruning so a watch never discloses a
-- broker-action removal id to a reader that could not see the original card.
ALTER TABLE elicitation_records ADD COLUMN broker_action INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cingulate_changes ADD COLUMN broker_action INTEGER NOT NULL DEFAULT 0;

DROP TRIGGER cingulate_change_card_insert;
DROP TRIGGER cingulate_change_card_update;
DROP TRIGGER cingulate_change_card_delete;
CREATE TRIGGER cingulate_change_card_insert AFTER INSERT ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding,config_plan,broker_action)
        VALUES (NEW.elicitation_id,NEW.secret_use,NEW.manifest_binding,NEW.config_plan,NEW.broker_action);
END;
CREATE TRIGGER cingulate_change_card_update AFTER UPDATE ON elicitation_records
WHEN OLD.state IS NOT NEW.state OR OLD.record_json IS NOT NEW.record_json
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding,config_plan,broker_action)
        VALUES (NEW.elicitation_id,NEW.secret_use,NEW.manifest_binding,NEW.config_plan,NEW.broker_action);
END;
CREATE TRIGGER cingulate_change_card_delete AFTER DELETE ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding,config_plan,broker_action)
        VALUES (OLD.elicitation_id,OLD.secret_use,OLD.manifest_binding,OLD.config_plan,OLD.broker_action);
END;
