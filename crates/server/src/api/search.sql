, anchored AS (
    -- Branches may share a request, and a long original may span several
    -- segments. Return its best passage once, with the corresponding branch.
    SELECT DISTINCT ON (p.message_id) x.*, p.message_id, m.session_id, m.role, m.origin, m.recorded_at,
           p.block_index, p.block_type, p.tool_name, s.title, s.harness, s.source_id, s.recorded_cwd,
           CASE WHEN $11 THEN coalesce(m.recorded_at, s.updated_at) END AS sort_time
    FROM ranked x JOIN search_passages p ON p.id = x.anchor
    JOIN messages m ON m.id = p.message_id JOIN sessions s ON s.id = m.session_id
    ORDER BY p.message_id, matched_words DESC, score DESC, leaf_id, is_tool, segment
), page AS MATERIALIZED (
    SELECT * FROM anchored
    ORDER BY sort_time DESC, matched_words DESC, score DESC, leaf_id, is_tool, segment
    LIMIT $12 OFFSET $13
)
SELECT coverage.*, querytree(k.query) AS positive, page.*, src.member, src.device_label,
       ARRAY(SELECT sp.project_id FROM session_projects sp
             WHERE sp.session_id = page.session_id ORDER BY sp.project_id) AS project_ids,
       ts_headline('english', p.body, k.query,
           E'MaxFragments=1, MaxWords=45, MinWords=12, ShortWord=2, StartSel=\x01, StopSel=\x02') AS headline
FROM coverage CROSS JOIN keywords k LEFT JOIN page ON true
LEFT JOIN sources src ON src.id = page.source_id
LEFT JOIN search_passages p ON p.id = page.anchor
ORDER BY sort_time DESC, matched_words DESC, score DESC, leaf_id, is_tool, segment
