WITH first AS (
    SELECT attempts FROM embedding_cache
    WHERE profile_id=$1 AND embedding IS NULL AND next_attempt<=now()
    ORDER BY next_attempt,digest LIMIT 1
), pending AS (
    SELECT digest FROM embedding_cache
    WHERE profile_id=$1 AND embedding IS NULL AND next_attempt<=now()
      AND EXISTS (SELECT FROM embedding_profiles WHERE id=$1 AND enabled)
    ORDER BY next_attempt,digest
    LIMIT (SELECT CASE WHEN attempts>0 THEN 1 ELSE $2::integer END FROM first)
    FOR UPDATE SKIP LOCKED
), claimed AS (
    UPDATE embedding_cache c SET owner=$3, next_attempt=now()+make_interval(secs => $4)
    FROM pending p WHERE c.profile_id=$1 AND c.digest=p.digest RETURNING c.digest
)
SELECT c.digest, coalesce(
    (SELECT p.body FROM embedding_passages p
     WHERE p.text_hash=c.digest AND ($5 OR NOT p.is_tool) LIMIT 1),
    (SELECT string_agg(p.body, E'\n\n' ORDER BY n.ord)
     FROM (SELECT embedding_passages FROM search_exchanges e
           WHERE e.embedding_hash=c.digest AND ($5 OR NOT e.is_tool) LIMIT 1) e
     CROSS JOIN LATERAL unnest(e.embedding_passages) WITH ORDINALITY n(id,ord)
     JOIN search_passages p USING (id))
) AS body FROM claimed c
