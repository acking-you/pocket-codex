-- Carry the client's device label from device-flow start through to the issued
-- refresh token. The column was already on refresh_tokens; this lets the label
-- supplied at `device_start` survive until the token is minted at poll time.
ALTER TABLE device_flows ADD COLUMN device_label TEXT;
