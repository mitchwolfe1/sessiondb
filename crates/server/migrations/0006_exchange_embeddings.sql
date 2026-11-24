-- Profile identity includes immutable model, transforms and document policy.
CREATE TABLE embedding_profiles (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    base_url text NOT NULL,
    model text NOT NULL,
    model_revision text NOT NULL,
    dimensions integer NOT NULL CHECK (dimensions BETWEEN 1 AND 16000),
    chunking_version integer NOT NULL DEFAULT 1 CHECK (chunking_version = 1),
    normalization text NOT NULL DEFAULT 'l2' CHECK (normalization = 'l2'),
    document_prefix text NOT NULL,
    query_prefix text NOT NULL,
    batch_size integer NOT NULL CHECK (batch_size BETWEEN 1 AND 256),
    timeout_seconds integer NOT NULL CHECK (timeout_seconds BETWEEN 1 AND 300),
    include_tools boolean NOT NULL,
    use_credential boolean NOT NULL,
    enabled boolean NOT NULL,
    UNIQUE (id, dimensions)
);
CREATE UNIQUE INDEX embedding_profiles_one_enabled ON embedding_profiles(enabled) WHERE enabled;

CREATE FUNCTION sessiondb_text_hash(body text) RETURNS bytea
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$ SELECT sha256(convert_to(body, 'UTF8')) $$;

ALTER TABLE search_passages ADD COLUMN text_hash bytea
    GENERATED ALWAYS AS (sessiondb_text_hash(body)) STORED;
CREATE INDEX search_passages_text_hash ON search_passages(text_hash);
ALTER TABLE search_exchanges ADD COLUMN embedding_hash bytea,
    ADD COLUMN embedding_passages bigint[] NOT NULL DEFAULT '{}';
CREATE INDEX search_exchanges_embedding_hash ON search_exchanges(embedding_hash)
    WHERE embedding_hash IS NOT NULL;

-- Keep exchange and member identities stable across a full projection rewrite.
CREATE VIEW embedding_passages AS
    SELECT p.*, m.session_id FROM search_passages p JOIN messages m ON m.id = p.message_id
    WHERE p.is_tool OR (m.role IN ('user','assistant') AND p.block_type = 'text');

CREATE FUNCTION sessiondb_exchange_embedding_input() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    SELECT coalesce(array_agg(p.id ORDER BY n.ord), '{}'),
           sessiondb_text_hash(string_agg(p.body, E'\n\n' ORDER BY n.ord))
    INTO NEW.embedding_passages, NEW.embedding_hash
    FROM unnest(NEW.passage_ids) WITH ORDINALITY n(id, ord) JOIN embedding_passages p USING (id);
    RETURN NEW;
END
$$;
CREATE TRIGGER search_exchange_embedding_input BEFORE INSERT OR UPDATE OF passage_ids ON search_exchanges
    FOR EACH ROW EXECUTE FUNCTION sessiondb_exchange_embedding_input();
UPDATE search_exchanges SET passage_ids = passage_ids;

CREATE VIEW embedding_sources AS
    SELECT p.session_id, p.is_tool, p.text_hash AS digest FROM embedding_passages p
    UNION ALL
    SELECT m.session_id, e.is_tool, e.embedding_hash FROM search_exchanges e
    JOIN messages m ON m.id = e.leaf_id WHERE e.embedding_hash IS NOT NULL;

-- Plain PostgreSQL still supports all keyword, sync and read operations.
-- Running migrate again after CREATE EXTENSION vector provisions the optional table.
CREATE FUNCTION sessiondb_prepare_vector_storage() RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    LOCK TABLE embedding_profiles IN EXCLUSIVE MODE;
    IF EXISTS (SELECT FROM pg_extension WHERE extname = 'vector')
       AND to_regclass('embedding_cache') IS NULL THEN
        CREATE TABLE embedding_cache (
            profile_id uuid NOT NULL, dimensions integer NOT NULL, digest bytea NOT NULL,
            embedding vector, next_attempt timestamptz DEFAULT now(), attempts integer NOT NULL DEFAULT 0,
            owner uuid, PRIMARY KEY (profile_id, digest),
            FOREIGN KEY (profile_id, dimensions) REFERENCES embedding_profiles(id, dimensions) ON DELETE CASCADE,
            CHECK (embedding IS NULL OR vector_dims(embedding) = dimensions),
            CHECK ((embedding IS NULL) = (next_attempt IS NOT NULL))
        );
        CREATE INDEX embedding_cache_pending ON embedding_cache(profile_id, next_attempt, digest)
            WHERE embedding IS NULL;
    END IF;
END
$$;

CREATE FUNCTION sessiondb_enqueue_embeddings(selected_sessions uuid[]) RETURNS void LANGUAGE plpgsql AS $$
DECLARE profile embedding_profiles%ROWTYPE;
BEGIN
    -- Profile activation seeds the archive under the conflicting EXCLUSIVE lock.
    -- A racing upload either appears in that scan or queues its own committed text.
    LOCK TABLE embedding_profiles IN ROW SHARE MODE;
    SELECT * INTO profile FROM embedding_profiles WHERE enabled;
    IF FOUND THEN
        INSERT INTO embedding_cache(profile_id, dimensions, digest)
        SELECT DISTINCT profile.id, profile.dimensions, s.digest FROM embedding_sources s
        WHERE (selected_sessions IS NULL OR s.session_id = ANY(selected_sessions))
          AND (profile.include_tools OR NOT s.is_tool)
        ORDER BY s.digest ON CONFLICT DO NOTHING;
    END IF;
END
$$;
SELECT sessiondb_prepare_vector_storage();
