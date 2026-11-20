-- Native tool output and provider/event payloads can contain valid JSON escapes
-- such as \u0000, which PostgreSQL JSONB cannot represent. Preserve these values
-- in JSON rather than rejecting valid records or scrubbing original content.
-- Typed relation/source-reference metadata remains JSONB; no payload indexes rely
-- on JSONB operators. Existing values are converted without changing their data.
ALTER TABLE messages
    ALTER COLUMN content TYPE json USING content::json,
    ALTER COLUMN provider_usage TYPE json USING provider_usage::json;

ALTER TABLE session_events
    ALTER COLUMN data TYPE json USING data::json;
