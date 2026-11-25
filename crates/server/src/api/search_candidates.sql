WITH parsed AS (
    SELECT websearch_to_tsquery('english', $1) AS query,
           tsvector_to_array(to_tsvector('english', $1)) AS words,
           replace(plainto_tsquery('english', $1)::text, ' & ', ' | ')::tsquery AS any_term
), keywords AS (
    SELECT CASE WHEN $14 THEN query ELSE any_term END AS query, any_term, words FROM parsed
), scoped_sessions AS NOT MATERIALIZED (
    SELECT s.* FROM sessions s
    WHERE (cardinality($2::uuid[]) = 0 OR EXISTS (
              SELECT FROM session_projects sp WHERE sp.session_id = s.id AND sp.project_id = ANY($2)))
      AND (NOT $3 OR NOT EXISTS (SELECT FROM session_projects sp WHERE sp.session_id = s.id))
      AND (cardinality($4::text[]) = 0 OR s.harness = ANY($4))
      AND (cardinality($5::text[]) = 0 OR s.tags @> $5)
      AND (($15::text IS NULL AND $16::text IS NULL) OR s.source_id IN (
            SELECT src.id FROM sources src WHERE ($15 IS NULL OR src.member=$15)
              AND ($16 IS NULL OR src.device_label=$16)))
), eligible AS NOT MATERIALIZED (
    SELECT p.*, m.session_id, m.role, m.origin, m.recorded_at, s.updated_at,
           m.sequence
    FROM search_passages p JOIN messages m ON m.id = p.message_id
    JOIN scoped_sessions s ON s.id = m.session_id
    WHERE (cardinality($6::text[]) = 0 OR m.role = ANY($6))
      AND ($7 OR NOT p.is_tool)
      AND ($8::text IS NULL OR p.tool_name = $8)
      AND ($9::timestamptz IS NULL OR coalesce(m.recorded_at, s.updated_at) >= $9)
      AND ($10::timestamptz IS NULL OR coalesce(m.recorded_at, s.updated_at) < $10)
), candidates AS (
    SELECT e.*, k.query, k.any_term, k.words
    FROM search_exchanges e CROSS JOIN keywords k
    JOIN messages leaf ON leaf.id = e.leaf_id
    JOIN scoped_sessions s ON s.id = leaf.session_id
    -- The positive subtree is a necessary condition even when a role or date
    -- filter removes a passage containing an excluded word.
    WHERE e.terms @@ nullif(nullif(querytree(k.query), 'T'), '')::tsquery
      AND ($7 OR NOT e.is_tool) AND ($8::text IS NULL OR e.is_tool)
), matches AS (
    SELECT c.leaf_id, c.is_tool, c.segment, a.anchor,
           CASE WHEN $14 THEN 0 ELSE length(a.terms) - length(ts_delete(a.terms, c.words)) END AS matched_words,
           ts_rank_cd(a.terms, c.query, 1) AS score
    FROM candidates c CROSS JOIN LATERAL (
        SELECT (array_agg(p.id ORDER BY (p.terms @@ c.query) DESC,
                    length(p.terms) - length(ts_delete(p.terms, c.words)) DESC,
                    ts_rank_cd(p.terms, c.query) DESC, ts_rank(p.terms, c.any_term) DESC,
                    p.sequence DESC, p.block_index, p.piece)
                FILTER (WHERE p.terms @@ c.any_term))[1] AS anchor,
               CASE WHEN count(*) = cardinality(c.passage_ids) THEN c.terms
                    ELSE to_tsvector('english', string_agg(p.body, E'\n' ORDER BY n.ord))
               END AS terms
        FROM unnest(c.passage_ids) WITH ORDINALITY n(id, ord)
        JOIN eligible p ON p.id = n.id
    ) a
    WHERE a.anchor IS NOT NULL AND a.terms @@ c.query
)
