-- No foreign key: feed retention must not erase an unpublished wake-up.
CREATE TABLE card_settled_outbox (
    elicitation_id TEXT NOT NULL,
    reader TEXT NOT NULL CHECK(reader IN ('aft','plexus','basal','broca','prefrontal-core')),
    feed TEXT NOT NULL CHECK(feed IN ('answers','outcomes')),
    seq INTEGER NOT NULL,
    PRIMARY KEY(elicitation_id,reader,seq)
);
CREATE INDEX card_settled_pending ON card_settled_outbox(reader,seq);
CREATE TABLE card_settled_counters (
    reader TEXT PRIMARY KEY,
    published INTEGER NOT NULL DEFAULT 0,
    dropped INTEGER NOT NULL DEFAULT 0
);
INSERT INTO card_settled_counters(reader) VALUES ('aft'),('plexus'),('basal'),('broca'),('prefrontal-core');
