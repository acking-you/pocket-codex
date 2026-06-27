-- Hosted-account backend schema.
--
-- Identity + credentials only; live service liveness comes from the relay, not
-- the DB. Timestamps are unix seconds (INTEGER) so the store needs no datetime
-- library and stays trivially portable.

CREATE TABLE users (
    -- Internal id, used as the per-user relay-key namespace (pcxu:<id>:...).
    -- Decoupled from github_id so the GitHub numeric id never leaks into keys.
    id            TEXT    PRIMARY KEY NOT NULL,
    github_id     INTEGER NOT NULL UNIQUE,
    github_login  TEXT    NOT NULL,
    created_at    INTEGER NOT NULL,
    last_login_at INTEGER NOT NULL
) STRICT;

CREATE TABLE refresh_tokens (
    id            TEXT    PRIMARY KEY NOT NULL,
    user_id       TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- SHA-256 of the opaque refresh token; the raw token is never stored.
    token_hash    BLOB    NOT NULL UNIQUE,
    device_label  TEXT,
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER NOT NULL,
    -- NULL while active; set when revoked or rotated.
    revoked_at    INTEGER,
    -- id of the token this one was rotated into (refresh-token rotation).
    rotated_to    TEXT
) STRICT;

CREATE INDEX idx_refresh_tokens_user ON refresh_tokens(user_id);

CREATE TABLE device_flows (
    -- Opaque handle handed to the client; the GitHub device_code stays server-side.
    handle             TEXT    PRIMARY KEY NOT NULL,
    github_device_code TEXT    NOT NULL,
    interval_secs      INTEGER NOT NULL,
    created_at         INTEGER NOT NULL,
    expires_at         INTEGER NOT NULL,
    -- NULL until the flow has been authorized and consumed.
    consumed_at        INTEGER
) STRICT;
