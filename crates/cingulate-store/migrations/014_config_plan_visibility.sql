-- Config-plan visibility must survive payload retention, including removals.
UPDATE access_asks SET record_json=cingulate_migration14_normalize_audit(record_json);
ALTER TABLE elicitation_records ADD COLUMN config_plan INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cingulate_changes ADD COLUMN config_plan INTEGER NOT NULL DEFAULT 0;
UPDATE elicitation_records SET config_plan=1
    WHERE json_valid(record_json) AND json_extract(record_json,'$.request.kind')='config_plan';

-- Older tombstones have no kind information. Discard their replay history so
-- old cursors reread rather than exposing an unclassified config-plan id.
UPDATE cingulate_change_log SET pruned_through=MAX(pruned_through,
    COALESCE((SELECT seq FROM sqlite_sequence WHERE name='cingulate_changes'),0));
DELETE FROM cingulate_changes;

DROP TRIGGER cingulate_change_card_insert;
DROP TRIGGER cingulate_change_card_update;
DROP TRIGGER cingulate_change_card_delete;
CREATE TRIGGER cingulate_change_card_insert AFTER INSERT ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding,config_plan)
        VALUES (NEW.elicitation_id,NEW.secret_use,NEW.manifest_binding,NEW.config_plan);
END;
CREATE TRIGGER cingulate_change_card_update AFTER UPDATE ON elicitation_records
WHEN OLD.state IS NOT NEW.state OR OLD.record_json IS NOT NEW.record_json
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding,config_plan)
        VALUES (NEW.elicitation_id,NEW.secret_use,NEW.manifest_binding,NEW.config_plan);
END;
CREATE TRIGGER cingulate_change_card_delete AFTER DELETE ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id,secret_use,manifest_binding,config_plan)
        VALUES (OLD.elicitation_id,OLD.secret_use,OLD.manifest_binding,OLD.config_plan);
END;
