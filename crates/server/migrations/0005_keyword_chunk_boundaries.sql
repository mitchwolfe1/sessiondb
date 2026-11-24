-- SUM(bigint) returns numeric, so the original bin expression divided into
-- fractional positions. SUM(integer) returns bigint and packs complete words
-- using integer division. Rebuild only the derived index and retain all anchors.
CREATE OR REPLACE FUNCTION sessiondb_search_chunks(input text) RETURNS SETOF text
LANGUAGE plpgsql IMMUTABLE STRICT AS $$
BEGIN
    IF octet_length(input) <= 8192 THEN
        RETURN NEXT input;
        RETURN;
    END IF;
    RETURN QUERY
        WITH words AS (
            SELECT word, ord, sum(octet_length(word) + 1) OVER (ORDER BY ord)
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

DROP INDEX search_exchanges_terms;
TRUNCATE search_passages, search_exchanges;
-- The per-session projection should look up that session's messages/passages
-- by index even while the new tables have no planner statistics.
SET LOCAL jit = off;
SET LOCAL enable_seqscan = off;
DO $$
DECLARE target uuid;
BEGIN
    FOR target IN SELECT id FROM sessions LOOP
        PERFORM sessiondb_index_session(target);
    END LOOP;
END
$$;
CREATE INDEX search_exchanges_terms ON search_exchanges USING gin(terms);
ANALYZE search_passages;
ANALYZE search_exchanges;
