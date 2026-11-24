, ranked AS (
    SELECT m.*, true AS by_keyword, false AS by_vector FROM matches m
), coverage AS (
    SELECT count(DISTINCT message_id) AS ready, 0::bigint AS eligible_count,
           0::bigint AS embedded_count, true AS profile_current FROM eligible
)
