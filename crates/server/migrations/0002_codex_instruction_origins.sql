-- Preserve every message and original; correct only derived Codex origins/titles.
-- The old default title was the first native-user text line, even when injected.
WITH message_blocks AS MATERIALIZED (
    SELECT m.id, m.session_id, m.sequence, block.ordinality,
           block.value->>'type' AS block_type,
           block.value->>'text' AS text,
           btrim(block.value->>'text', E' \t\n\r\013\f') AS trimmed
    FROM messages m
    JOIN sessions s ON s.id = m.session_id AND s.harness = 'codex'
    CROSS JOIN LATERAL jsonb_array_elements(m.content) WITH ORDINALITY AS block(value, ordinality)
    WHERE m.role = 'user' AND m.origin = 'native_user'
), instruction_messages AS MATERIALIZED (
    SELECT id,
           bool_or(block_type = 'text' AND trimmed <> '')
           AND bool_and(CASE WHEN block_type = 'text' THEN
               trimmed = ''
               OR trimmed LIKE '<environment_context>%</environment_context>'
               OR (
                   trimmed LIKE '# AGENTS.md instructions for %'
                   AND strpos(trimmed, E'\n') > 0
                   AND btrim(substr(split_part(trimmed, E'\n', 1), length('# AGENTS.md instructions for ') + 1)) <> ''
                   AND btrim(substr(trimmed, strpos(trimmed, E'\n') + 1), E' \t\n\r\013\f')
                       LIKE '<INSTRUCTIONS>%</INSTRUCTIONS>'
               )
           ELSE false END) AS injected
    FROM message_blocks
    GROUP BY id
), old_titles AS (
    SELECT DISTINCT ON (session_id) session_id,
           left(split_part(replace(text, E'\r\n', E'\n'), E'\n', 1), 160) AS title
    FROM message_blocks
    WHERE block_type = 'text'
    ORDER BY session_id, sequence, ordinality
), new_titles AS (
    SELECT DISTINCT ON (b.session_id) b.session_id,
           btrim(left(btrim(regexp_replace(line.value, '[[:cntrl:]]', ' ', 'g'), E' \t\n\r\013\f'), 160), E' \t\n\r\013\f') AS title
    FROM message_blocks b
    JOIN instruction_messages i ON i.id = b.id AND NOT i.injected
    CROSS JOIN LATERAL regexp_split_to_table(b.text, E'\r?\n') WITH ORDINALITY AS line(value, ordinality)
    WHERE b.block_type = 'text' AND btrim(regexp_replace(line.value, '[[:cntrl:]]', ' ', 'g'), E' \t\n\r\013\f') <> ''
    ORDER BY b.session_id, b.sequence, b.ordinality, line.ordinality
), corrected_origins AS (
    UPDATE messages m SET origin = 'harness'
    FROM instruction_messages i
    WHERE m.id = i.id AND i.injected
    RETURNING m.id
)
UPDATE sessions s SET title = coalesce(n.title, 'Untitled session')
FROM old_titles o LEFT JOIN new_titles n ON n.session_id = o.session_id
WHERE s.id = o.session_id AND s.title = o.title;
