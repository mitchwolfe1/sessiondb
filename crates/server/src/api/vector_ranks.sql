, active AS (
    SELECT id, include_tools FROM embedding_profiles WHERE id=$15 AND enabled
), query_vector AS (
    SELECT $16::real[]::vector AS value
), semantic_units AS MATERIALIZED (
    SELECT e.leaf_id, e.is_tool, e.segment, members.ids,
           CASE WHEN members.ids=e.embedding_passages THEN whole.embedding <=> q.value
                ELSE (SELECT min(c.embedding <=> q.value)
                      FROM unnest(members.ids) n(id) JOIN search_passages p USING(id)
                      JOIN embedding_cache c ON c.profile_id=a.id AND c.digest=p.text_hash)
           END AS distance
    FROM search_exchanges e JOIN messages leaf ON leaf.id=e.leaf_id
    JOIN scoped_sessions s ON s.id=leaf.session_id CROSS JOIN active a CROSS JOIN query_vector q
    CROSS JOIN LATERAL (
        SELECT array_agg(n.id ORDER BY n.ord) AS ids
        FROM unnest(e.embedding_passages) WITH ORDINALITY n(id,ord) JOIN eligible p USING(id)
    ) members
    LEFT JOIN embedding_cache whole ON whole.profile_id=a.id AND whole.digest=e.embedding_hash
    WHERE members.ids IS NOT NULL AND (NOT e.is_tool OR (a.include_tools AND $7))
), nearest AS (
    SELECT * FROM semantic_units WHERE distance IS NOT NULL
    ORDER BY distance, leaf_id, is_tool, segment LIMIT 200
), dense AS (
    SELECT n.leaf_id, n.is_tool, n.segment, member.id AS anchor,
           row_number() OVER (ORDER BY n.distance,n.leaf_id,n.is_tool,n.segment) AS rank
    FROM nearest n CROSS JOIN query_vector q CROSS JOIN keywords k
    CROSS JOIN LATERAL (
        SELECT p.id FROM unnest(n.ids) member(id) JOIN eligible p USING(id)
        LEFT JOIN embedding_cache c ON c.profile_id=$15 AND c.digest=p.text_hash
        ORDER BY c.embedding <=> q.value NULLS LAST, ts_rank(p.terms,k.any_term) DESC,
                 p.sequence DESC,p.block_index,p.piece LIMIT 1
    ) member
), lexical AS (
    SELECT m.*, row_number() OVER (ORDER BY matched_words DESC,score DESC,leaf_id,is_tool,segment) AS rank
    FROM matches m WHERE $17
    ORDER BY matched_words DESC,score DESC,leaf_id,is_tool,segment LIMIT 200
), ranked AS (
    SELECT leaf_id,is_tool,segment,coalesce(d.anchor,l.anchor) AS anchor,
           0 AS matched_words, coalesce(1.0/(60+d.rank),0)+coalesce(1.0/(60+l.rank),0) AS score,
           l.rank IS NOT NULL AS by_keyword,d.rank IS NOT NULL AS by_vector
    FROM lexical l FULL JOIN dense d USING (leaf_id,is_tool,segment)
), coverage AS (
    SELECT (SELECT count(DISTINCT message_id) FROM eligible) AS ready,
           count(*) AS eligible_count,count(distance) AS embedded_count,
           EXISTS(SELECT FROM active) AS profile_current FROM semantic_units
)
