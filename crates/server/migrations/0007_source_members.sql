-- Established sources acquire a member only when a client identifies the store.
-- Sessions already reference their collecting source, including pre-upgrade data.
ALTER TABLE sources ADD COLUMN member text
    CHECK (member COLLATE "C" ~ '^[a-z0-9][a-z0-9_-]{0,63}$');
