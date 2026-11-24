use super::*;
use sessiondb_types::{
    Role, SearchCoverage, SearchMatch, SearchMode, SearchRequest, SearchResponse, SearchSort,
};

#[derive(Serialize, Deserialize)]
struct SearchCursor {
    scope: String,
    offset: i64,
}

pub(super) async fn search(
    State(state): State<Arc<AppState>>,
    ApiJson(request): ApiJson<SearchRequest>,
) -> ApiResult<Json<SearchResponse>> {
    let query = request.query.trim();
    let filters = &request.filters;
    let size = request.limit.unwrap_or(3);
    if query.is_empty() || query.contains('\0') || request.query.chars().count() > 2000 {
        return Err(ApiError::bad(
            "invalid_query",
            "Supply 1 to 2000 characters of search text.",
        ));
    }
    if !(1..=50).contains(&size) {
        return Err(ApiError::bad(
            "invalid_limit",
            "limit must be between 1 and 50.",
        ));
    }
    if (filters.unassigned && !filters.project_ids.is_empty())
        || filters
            .after
            .zip(filters.before)
            .is_some_and(|(a, b)| a >= b)
        || filters
            .tags
            .iter()
            .chain(&filters.tool_name)
            .any(|s| s.contains('\0'))
    {
        return Err(ApiError::bad(
            "invalid_filter",
            "The selected filters are invalid or incompatible.",
        ));
    }
    if request.mode != SearchMode::Keyword {
        if request.sort == SearchSort::Newest {
            return Err(ApiError::bad(
                "invalid_sort",
                "Newest ordering requires keyword mode.",
            ));
        }
        return Err(ApiError::conflict(
            "embedding_not_configured",
            "This instance has no enabled embedding profile.",
        ));
    }
    let scope = scope(("search", query, filters, request.mode, request.sort))?;
    let offset = match request.cursor.as_deref() {
        Some(value) => {
            let cursor: SearchCursor = decode_position(value)?;
            if cursor.scope != scope || cursor.offset < 0 || cursor.offset > i64::MAX - 51 {
                return Err(ApiError::bad(
                    "invalid_cursor",
                    "The cursor does not match this search.",
                ));
            }
            cursor.offset
        }
        None => 0,
    };
    let tools =
        filters.include_tools || filters.tool_name.is_some() || filters.roles.contains(&Role::Tool);
    let operators = query.contains('"')
        || query
            .split_whitespace()
            .any(|word| matches!(word, "AND" | "OR") || (word.starts_with('-') && word.len() > 1));
    let rows = sqlx::query(include_str!("search.sql"))
        .bind(query)
        .bind(&filters.project_ids)
        .bind(filters.unassigned)
        .bind(
            filters
                .harnesses
                .iter()
                .map(|v| v.as_str())
                .collect::<Vec<_>>(),
        )
        .bind(&filters.tags)
        .bind(filters.roles.iter().map(|v| v.as_str()).collect::<Vec<_>>())
        .bind(tools)
        .bind(&filters.tool_name)
        .bind(filters.after)
        .bind(filters.before)
        .bind(request.sort == SearchSort::Newest)
        .bind(i64::from(size) + 1)
        .bind(offset)
        .bind(operators)
        .fetch_all(&state.pool)
        .await?;
    // Coverage is returned even for an empty page, in the same statement snapshot.
    let first = &rows[0];
    if matches!(first.try_get::<&str, _>("positive")?, "" | "T") {
        return Err(ApiError::bad(
            "invalid_query",
            "Each alternative must require a positive searchable term.",
        ));
    }
    let mut data = Vec::new();
    for row in rows.iter().take(size as usize) {
        let Some(id) = row.try_get::<Option<Uuid>, _>("message_id")? else {
            break;
        };
        let leaf_id: Uuid = row.try_get("leaf_id")?;
        let mut show_command = format!("sessiondb messages show {id} -C 2");
        if leaf_id != id {
            show_command.push_str(&format!(" --leaf {leaf_id}"));
        }
        data.push(SearchMatch {
            message_id: id,
            session_id: row.try_get("session_id")?,
            leaf_id,
            show_command,
            role: row
                .try_get::<&str, _>("role")?
                .parse()
                .map_err(ApiError::internal)?,
            origin: row
                .try_get::<&str, _>("origin")?
                .parse()
                .map_err(ApiError::internal)?,
            harness: row
                .try_get::<&str, _>("harness")?
                .parse()
                .map_err(ApiError::internal)?,
            block_type: row.try_get("block_type")?,
            tool_name: row.try_get("tool_name")?,
            block_index: row.try_get::<i32, _>("block_index")? as u32,
            timestamp: row.try_get("recorded_at")?,
            title: row.try_get("title")?,
            project_ids: row.try_get("project_ids")?,
            retrieved_by: vec![SearchMode::Keyword],
            excerpt: excerpt(row.try_get("headline")?),
            url: format!(
                "{}/v1/messages/{id}",
                state.config.public_url.as_str().trim_end_matches('/')
            ),
        });
    }
    let next_cursor = (rows.len() > size as usize)
        .then(|| {
            encode_cursor(SearchCursor {
                scope,
                offset: offset + i64::from(size),
            })
        })
        .transpose()?;
    Ok(Json(SearchResponse {
        data,
        next_cursor,
        mode: SearchMode::Keyword,
        coverage: SearchCoverage {
            keyword_ready_messages: first.try_get::<i64, _>("ready")? as u64,
            embedding_eligible_passages: 0,
            embedded_passages: 0,
            embedding_profile_id: None,
        },
    }))
}

// ts_headline bounds words. Also bound characters when a word is a long path or identifier.
fn excerpt(headline: &str) -> String {
    const LIMIT: usize = 480;
    let position = headline
        .split_once('\u{1}')
        .map_or(0, |(prefix, _)| prefix.chars().count());
    let plain: Vec<_> = headline
        .chars()
        .filter(|c| !matches!(c, '\u{1}' | '\u{2}'))
        .collect();
    if plain.len() <= LIMIT {
        return plain.into_iter().collect();
    }
    let start = position
        .saturating_sub(LIMIT / 3)
        .min(plain.len() - LIMIT + 2);
    let mut result = String::new();
    if start > 0 {
        result.push('…');
    }
    result.extend(&plain[start..start + LIMIT - 2]);
    if start + LIMIT - 2 < plain.len() {
        result.push('…');
    }
    result
}
