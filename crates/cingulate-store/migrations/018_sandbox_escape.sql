-- Keep the call-to-card mapping after pruning the command so that a delayed
-- retry cannot open another one-time approval.
CREATE TABLE sandbox_escape_requests (
    owner TEXT NOT NULL,
    call_key TEXT NOT NULL,
    elicitation_id TEXT NOT NULL,
    PRIMARY KEY (owner, call_key)
);

-- A watch must not reveal removal ids for cards its reader could never see.
ALTER TABLE elicitation_records ADD COLUMN sandbox_escape INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cingulate_changes ADD COLUMN sandbox_escape INTEGER NOT NULL DEFAULT 0;

DROP TRIGGER cingulate_change_card_insert;
DROP TRIGGER cingulate_change_card_update;
DROP TRIGGER cingulate_change_card_delete;
CREATE TRIGGER cingulate_change_card_insert AFTER INSERT ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding,config_plan,broker_action,sandbox_escape)
        VALUES (NEW.elicitation_id,NEW.secret_use,NEW.manifest_binding,NEW.config_plan,NEW.broker_action,NEW.sandbox_escape);
END;
CREATE TRIGGER cingulate_change_card_update AFTER UPDATE ON elicitation_records
WHEN OLD.state IS NOT NEW.state OR OLD.record_json IS NOT NEW.record_json
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding,config_plan,broker_action,sandbox_escape)
        VALUES (NEW.elicitation_id,NEW.secret_use,NEW.manifest_binding,NEW.config_plan,NEW.broker_action,NEW.sandbox_escape);
END;
CREATE TRIGGER cingulate_change_card_delete AFTER DELETE ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding,config_plan,broker_action,sandbox_escape)
        VALUES (OLD.elicitation_id,OLD.secret_use,OLD.manifest_binding,OLD.config_plan,OLD.broker_action,OLD.sandbox_escape);
END;
