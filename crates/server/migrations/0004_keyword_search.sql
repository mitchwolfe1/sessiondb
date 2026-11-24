-- These projections can be rebuilt entirely from accepted normalized messages.
-- The leaf identifies one continuation of a request across assistant/tool turns.
CREATE INDEX messages_children ON messages(conversation_parent_id, sequence);

CREATE TABLE search_passages (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    message_id uuid NOT NULL REFERENCES messages ON DELETE CASCADE,
    block_index integer NOT NULL,
    piece bigint NOT NULL,
    block_type text NOT NULL,
    is_tool boolean NOT NULL,
    tool_name text,
    body text NOT NULL,
    terms tsvector GENERATED ALWAYS AS (to_tsvector('english', body)) STORED,
    UNIQUE (message_id, block_index, piece)
);

CREATE TABLE search_exchanges (
    leaf_id uuid NOT NULL REFERENCES messages ON DELETE CASCADE,
    is_tool boolean NOT NULL,
    segment bigint NOT NULL,
    passage_ids bigint[] NOT NULL,
    terms tsvector NOT NULL,
    PRIMARY KEY (leaf_id, is_tool, segment)
);

-- Prefer intact blocks. For a large block, pack complete words into passages.
-- The final fallback bounds individual unbroken strings such as encoded data.
CREATE FUNCTION sessiondb_search_chunks(input text) RETURNS SETOF text
LANGUAGE plpgsql IMMUTABLE STRICT AS $$
BEGIN
    IF octet_length(input) <= 8192 THEN
        RETURN NEXT input;
        RETURN;
    END IF;
    RETURN QUERY
        WITH words AS (
            SELECT word, ord, sum(octet_length(word)::bigint + 1) OVER (ORDER BY ord)
                - octet_length(word) - 1 AS pos
            FROM regexp_split_to_table(input, '[[:space:]]+') WITH ORDINALITY w(word, ord)
            WHERE word <> ''
        ), packed AS (
            SELECT pos / 8192 AS bin, string_agg(word, ' ' ORDER BY ord) AS body
            FROM words GROUP BY pos / 8192
        )
        SELECT substr(body, start, 16384)
        FROM packed CROSS JOIN LATERAL generate_series(1, length(body), 16384) start
        ORDER BY bin, start;
END
$$;

-- Called once per published session, inside the existing acceptance transaction.
-- Publication replaces that session's messages; their FKs remove both old projections.
CREATE FUNCTION sessiondb_index_session(target uuid) RETURNS void
LANGUAGE plpgsql AS $$
BEGIN
    WITH blocks AS MATERIALIZED (
        SELECT m.id, m.role, (b.ord - 1)::integer AS block_index, b.value,
               b.value->>'type' AS kind
        FROM messages m CROSS JOIN LATERAL json_array_elements(
            -- PostgreSQL text cannot contain NUL. Decode only a display copy,
            -- preserving even backslashes (literal \u0000) and the original json.
            CASE WHEN strpos(m.content::text, '\u0000') > 0 THEN
                regexp_replace(m.content::text, '(?<!\\)((?:\\\\)*)\\u0000', '\1 ', 'g')::json
                ELSE m.content END
        ) WITH ORDINALITY b(value,ord)
        WHERE m.session_id = target
    ), calls AS (
        SELECT value->>'call_id' AS call_id, min(value->>'name') AS name
        FROM blocks WHERE kind = 'tool_call'
        GROUP BY value->>'call_id' HAVING count(*) = 1
    ), texts AS (
        SELECT b.*, CASE kind
            WHEN 'text' THEN value->>'text'
            WHEN 'json' THEN value->>'value'
            WHEN 'artifact' THEN value->>'alt'
            WHEN 'tool_call' THEN concat_ws(' ', value->>'name', value->'input'->>'value')
            WHEN 'tool_result' THEN (
                SELECT string_agg(coalesce(o->>'text', o->>'value', o->>'alt'), E'\n' ORDER BY ord)
                FROM json_array_elements(value->'content') WITH ORDINALITY outputs(o, ord)
            )
        END AS body, CASE WHEN kind = 'tool_call' THEN value->>'name' ELSE c.name END AS tool_name
        FROM blocks b LEFT JOIN calls c ON kind = 'tool_result' AND c.call_id = value->>'call_id'
    )
    INSERT INTO search_passages(message_id, block_index, piece, block_type, is_tool, tool_name, body)
    SELECT id, block_index, chunk.ord, kind, role = 'tool' OR kind IN ('tool_call','tool_result'),
           tool_name, chunk.body
    FROM texts CROSS JOIN LATERAL sessiondb_search_chunks(texts.body) WITH ORDINALITY chunk(body, ord)
    WHERE chunk.body ~ '[^[:space:]]';

    WITH RECURSIVE paths(leaf_id, id, parent, role, sequence) AS (
        -- End a turn at a leaf or before a new request/standalone message.
        SELECT m.id, m.id, m.conversation_parent_id, m.role, m.sequence
        FROM messages m WHERE m.session_id = target AND (
            NOT EXISTS (SELECT FROM messages c WHERE c.conversation_parent_id = m.id
                        AND c.session_id = target)
            OR EXISTS (SELECT FROM messages c WHERE c.conversation_parent_id = m.id
                       AND c.session_id = target AND c.role NOT IN ('assistant','tool'))
            OR m.role NOT IN ('user','assistant','tool')
        )
        UNION ALL
        SELECT child.leaf_id, m.id, m.conversation_parent_id, m.role, m.sequence
        FROM paths child JOIN messages m ON m.id = child.parent
        WHERE child.role IN ('assistant','tool') AND m.role IN ('user','assistant','tool')
          AND m.session_id = target AND m.sequence < child.sequence
    ), memberships AS (
        SELECT leaf_id, id, bool_or(role = 'user') OVER (PARTITION BY leaf_id) AS has_request
        FROM paths
    ), members AS (
        -- Without a user request, give each standalone message its own exchange.
        SELECT DISTINCT CASE WHEN has_request THEN leaf_id ELSE id END AS leaf_id, id
        FROM memberships
    ), parts AS (
        SELECT m.leaf_id, p.id, p.is_tool, p.body, msg.sequence, p.block_index, p.piece
        FROM members m JOIN messages msg ON msg.id = m.id
        JOIN search_passages p ON p.message_id = m.id AND NOT p.is_tool
        UNION ALL
        SELECT msg.id, p.id, true, p.body, msg.sequence, p.block_index, p.piece
        FROM messages msg JOIN search_passages p ON p.message_id = msg.id
        WHERE msg.session_id = target AND p.is_tool
    ), segments AS (
        SELECT *, (sum(octet_length(body)::bigint + 1) OVER (
            PARTITION BY leaf_id, is_tool ORDER BY sequence, block_index, piece
        ) - octet_length(body) - 1)::bigint / 16384 AS segment FROM parts
    )
    INSERT INTO search_exchanges(leaf_id, is_tool, segment, passage_ids, terms)
    SELECT leaf_id, is_tool, segment, array_agg(id ORDER BY sequence, block_index, piece),
           to_tsvector('english', string_agg(body, E'\n' ORDER BY sequence, block_index, piece))
    FROM segments GROUP BY leaf_id, is_tool, segment;
END
$$;

-- Bound working sets to one session, including on deployed catalogs.
DO $$
DECLARE target uuid;
BEGIN
    FOR target IN SELECT id FROM sessions LOOP
        PERFORM sessiondb_index_session(target);
    END LOOP;
END
$$;
CREATE INDEX search_exchanges_terms ON search_exchanges USING gin(terms);
