-- Web (authorization-code / browser-redirect) login flow.
--
-- The backend brokers GitHub's authorization-code flow (it holds the client
-- secret): `web_auth_flows` tracks an in-flight browser round-trip; on the
-- GitHub callback the backend mints a single-use `web_exchange_codes` row that
-- the client trades for a session at /auth/web/exchange. The device flow
-- (device_flows) is untouched — both converge on the same users/refresh_tokens.

CREATE TABLE web_auth_flows (
    -- Opaque flow id (primary key); the GitHub `state` keys the callback lookup.
    flow_id        TEXT    PRIMARY KEY NOT NULL,
    -- Random state echoed to GitHub and returned on the callback, so we can find
    -- this flow and reject a forged callback (CSRF). UNIQUE: a duplicate fails.
    gh_state       TEXT    NOT NULL UNIQUE,
    -- Where to send the browser at the end: the app's custom-scheme deep link
    -- (pocketcodex://…) or a loopback http URL. Validated against an allowlist
    -- at start, so the one-time exchange code can never be redirected off-device.
    redirect_uri   TEXT    NOT NULL,
    -- The client's own CSRF state, echoed back to it in the final redirect.
    app_state      TEXT    NOT NULL,
    -- base64url(SHA-256(code_verifier)) — PKCE binding for the app↔backend leg,
    -- so only the client that started the flow can redeem the exchange code
    -- (defends against custom-scheme hijacking on mobile).
    code_challenge TEXT    NOT NULL,
    -- Optional device label, carried onto the eventual refresh token.
    device_label   TEXT,
    created_at     INTEGER NOT NULL,
    expires_at     INTEGER NOT NULL,
    -- NULL until the GitHub callback consumes it (authorize-once).
    consumed_at    INTEGER
) STRICT;

CREATE INDEX idx_web_auth_flows_state ON web_auth_flows(gh_state);

CREATE TABLE web_exchange_codes (
    -- One-time code delivered to the client via the final redirect; traded (with
    -- the PKCE verifier) for a session at /auth/web/exchange.
    code           TEXT    PRIMARY KEY NOT NULL,
    user_id        TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Device label to carry onto the issued refresh token.
    device_label   TEXT,
    -- PKCE challenge copied from the flow; the exchange verifies the verifier.
    code_challenge TEXT    NOT NULL,
    created_at     INTEGER NOT NULL,
    expires_at     INTEGER NOT NULL,
    -- NULL until redeemed (redeem-once).
    consumed_at    INTEGER
) STRICT;
