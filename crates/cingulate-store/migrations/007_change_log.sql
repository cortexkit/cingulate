-- The change log behind consent.watch_pending. Every committed change to a
-- card the phone can see (a generic card in elicitation_records or a
-- permission ask in access_asks) appends one row in the same transaction, so a
-- watch can replay exactly the changes after a cursor and never miss one.
-- Triggers write the rows, so no mutation path can forget to.
--
-- A table rebuild drops its triggers: a later migration that rebuilds
-- elicitation_records or access_asks must recreate the triggers below.
CREATE TABLE cingulate_changes (
    version INTEGER PRIMARY KEY AUTOINCREMENT,
    elicitation_id TEXT NOT NULL
);
CREATE INDEX cingulate_changes_card ON cingulate_changes(elicitation_id, version);

-- `instance` names this store file, so a cursor minted by another store (a
-- reinstall, another machine) is recognized as foreign. `pruned_through` is
-- the highest version whose row compaction deleted: a cursor below it can no
-- longer be replayed in full and must be answered with a reread.
CREATE TABLE cingulate_change_log (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    instance TEXT NOT NULL,
    pruned_through INTEGER NOT NULL CHECK (typeof(pruned_through) = 'integer' AND pruned_through >= 0)
);
INSERT INTO cingulate_change_log (singleton, instance, pruned_through)
    VALUES (1, lower(hex(randomblob(16))), 0);
-- Start the version sequence at zero so an empty log still has a head.
INSERT INTO sqlite_sequence (name, seq) SELECT 'cingulate_changes', 0
    WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'cingulate_changes');

CREATE TRIGGER cingulate_change_card_insert AFTER INSERT ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id) VALUES (NEW.elicitation_id);
END;
-- Only state and the stored record shape the phone projection. Retention
-- clearing record_json is a change too: the card leaves the phone's view.
CREATE TRIGGER cingulate_change_card_update AFTER UPDATE ON elicitation_records
WHEN OLD.state IS NOT NEW.state OR OLD.record_json IS NOT NEW.record_json
BEGIN
    INSERT INTO cingulate_changes (elicitation_id) VALUES (NEW.elicitation_id);
END;
CREATE TRIGGER cingulate_change_card_delete AFTER DELETE ON elicitation_records
BEGIN
    INSERT INTO cingulate_changes (elicitation_id) VALUES (OLD.elicitation_id);
END;
CREATE TRIGGER cingulate_change_ask_insert AFTER INSERT ON access_asks
BEGIN
    INSERT INTO cingulate_changes (elicitation_id) VALUES (NEW.elicitation_id);
END;
CREATE TRIGGER cingulate_change_ask_update AFTER UPDATE ON access_asks
WHEN OLD.state IS NOT NEW.state OR OLD.record_json IS NOT NEW.record_json
BEGIN
    INSERT INTO cingulate_changes (elicitation_id) VALUES (NEW.elicitation_id);
END;
CREATE TRIGGER cingulate_change_ask_delete AFTER DELETE ON access_asks
BEGIN
    INSERT INTO cingulate_changes (elicitation_id) VALUES (OLD.elicitation_id);
END;
