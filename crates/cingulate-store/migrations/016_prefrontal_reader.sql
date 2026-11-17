-- Published pointers are removed from the outbox; all remaining rows are pending.
-- Keep the old reader in the CHECK so a rollback can still enqueue its pointers.
CREATE TABLE card_settled_outbox_renamed (
    elicitation_id TEXT NOT NULL,
    reader TEXT NOT NULL CHECK(reader IN ('aft','plexus','basal','broca','prefrontal','prefrontal-core')),
    feed TEXT NOT NULL CHECK(feed IN ('answers','outcomes')),
    seq INTEGER NOT NULL,
    PRIMARY KEY(elicitation_id,reader,seq)
);
INSERT INTO card_settled_outbox_renamed(elicitation_id,reader,feed,seq)
    SELECT elicitation_id,
           CASE reader WHEN 'prefrontal-core' THEN 'prefrontal' ELSE reader END,
           feed,seq
    FROM card_settled_outbox;
DROP TABLE card_settled_outbox;
ALTER TABLE card_settled_outbox_renamed RENAME TO card_settled_outbox;
CREATE INDEX card_settled_pending ON card_settled_outbox(reader,seq);
UPDATE card_settled_counters SET reader='prefrontal' WHERE reader='prefrontal-core';
