CREATE TABLE cingulate_id_seq (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    value INTEGER NOT NULL CHECK (value >= 0)
);
INSERT INTO cingulate_id_seq (singleton, value) VALUES (1, 0);
