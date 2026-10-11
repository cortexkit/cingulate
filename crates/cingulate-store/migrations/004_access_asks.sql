CREATE TABLE access_asks (
    elicitation_id TEXT PRIMARY KEY,
    principal TEXT NOT NULL,
    owner TEXT NOT NULL,
    call_key TEXT,
    folder TEXT,
    program TEXT,
    state TEXT NOT NULL,
    record_json TEXT NOT NULL
);
CREATE UNIQUE INDEX access_call_key ON access_asks(principal, call_key) WHERE call_key IS NOT NULL;
CREATE UNIQUE INDEX access_macos_open ON access_asks(folder, program) WHERE folder IS NOT NULL AND state = 'pending:user';
CREATE TABLE access_notices (
    elicitation_id TEXT NOT NULL REFERENCES access_asks(elicitation_id),
    purpose TEXT NOT NULL,
    body_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    retry_at_ms INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    PRIMARY KEY (elicitation_id, purpose)
);
CREATE TRIGGER access_insert_revision AFTER INSERT ON access_asks BEGIN
    UPDATE cingulate_pending_revision SET value=value+1 WHERE singleton=1;
END;
CREATE TRIGGER access_update_revision AFTER UPDATE ON access_asks BEGIN
    UPDATE cingulate_pending_revision SET value=value+1 WHERE singleton=1;
END;
