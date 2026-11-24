WITH RECURSIVE anchor AS (
    SELECT id, session_id, conversation_parent_id, sequence FROM messages WHERE id = $1
), chosen AS (
    SELECT m.id, m.conversation_parent_id, m.sequence
    FROM messages m JOIN anchor a USING (session_id) WHERE m.id = $2
    UNION ALL
    SELECT p.id, p.conversation_parent_id, p.sequence
    FROM chosen c JOIN messages p ON p.id = c.conversation_parent_id
    JOIN anchor a ON a.session_id = p.session_id
    WHERE c.id <> a.id AND p.sequence < c.sequence
), preceding AS (
    SELECT a.*, 0 AS depth FROM anchor a
    UNION ALL
    SELECT p.id, p.session_id, p.conversation_parent_id, p.sequence, c.depth - 1
    FROM preceding c JOIN messages p ON p.id = c.conversation_parent_id
    WHERE p.session_id = c.session_id AND p.sequence < c.sequence AND c.depth > -$3
), following AS (
    SELECT a.*, 0 AS depth FROM anchor a
    UNION ALL
    SELECT child.id, f.session_id, f.id, child.sequence, f.depth + 1
    FROM following f CROSS JOIN LATERAL (
        SELECT m.id, m.sequence, count(*) OVER () AS siblings FROM messages m
        WHERE m.conversation_parent_id = f.id AND m.session_id = f.session_id AND m.sequence > f.sequence
    ) child
    WHERE f.depth < $4 AND (child.siblings = 1 OR child.id IN (SELECT id FROM chosen))
), neighbors AS (
    SELECT id, depth FROM preceding UNION ALL SELECT id, depth FROM following WHERE depth > 0
), last_message AS (
    SELECT id, session_id FROM following ORDER BY depth DESC LIMIT 1
), choices AS (
    SELECT coalesce(array_agg(m.id ORDER BY m.sequence) FILTER (WHERE m.id IS NOT NULL), '{}') AS ids
    FROM last_message l JOIN messages m ON m.conversation_parent_id = l.id AND m.session_id = l.session_id
    HAVING count(*) > 1 AND NOT EXISTS (
        SELECT FROM chosen c JOIN last_message l ON c.conversation_parent_id = l.id
    )
)
SELECT m.*, ($2::uuid IS NULL OR EXISTS (SELECT FROM chosen WHERE id = $1)) AS valid_branch,
       coalesce((SELECT ids FROM choices), '{}') AS choices
FROM neighbors w JOIN messages m ON m.id = w.id ORDER BY w.depth
