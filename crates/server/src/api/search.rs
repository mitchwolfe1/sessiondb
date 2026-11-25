use super::*;
use sessiondb_types::{
    MemberHandle, Role, SearchCoverage, SearchMatch, SearchMode, SearchRequest, SearchResponse,
    SearchSort,
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
            .device_label
            .as_deref()
            .is_some_and(|label| !sources::valid_device(label))
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
    let profile = if request.mode == SearchMode::Keyword {
        None
    } else {
        if request.sort == SearchSort::Newest {
            return Err(ApiError::bad(
                "invalid_sort",
                "Newest ordering requires keyword mode.",
            ));
        }
        Some(
            crate::embeddings::enabled(&state.pool)
                .await?
                .ok_or_else(|| {
                    ApiError::conflict(
                        "embedding_not_configured",
                        "This instance has no enabled embedding profile.",
                    )
                })?,
        )
    };
    let request_scope = ("search", query, filters, request.mode, request.sort);
    let scope = match &profile {
        Some(profile) => scope((request_scope, profile.id))?,
        None => scope(request_scope)?,
    };
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
    const KEYWORD: &str = concat!(
        include_str!("search_candidates.sql"),
        include_str!("keyword_ranks.sql"),
        include_str!("search.sql")
    );
    const VECTOR: &str = concat!(
        include_str!("search_candidates.sql"),
        include_str!("vector_ranks.sql"),
        include_str!("search.sql")
    );
    let mut statement = sqlx::query(if profile.is_some() { VECTOR } else { KEYWORD })
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
        .bind(filters.member.as_ref().map(MemberHandle::as_str))
        .bind(&filters.device_label);
    if let Some(profile) = &profile {
        let vector = state
            .embedding_client
            .query(profile, query)
            .await
            .map_err(|error| {
                tracing::warn!(%error, "query embedding unavailable");
                ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "embedding_provider_unavailable",
                    "The configured provider could not embed this query.",
                )
            })?;
        statement = statement
            .bind(profile.id)
            .bind(vector)
            .bind(request.mode == SearchMode::Hybrid);
    }
    let rows = statement.fetch_all(&state.pool).await?;
    // Coverage is returned even for an empty page, in the same statement snapshot.
    let first = &rows[0];
    if !first.try_get::<bool, _>("profile_current")? {
        return Err(ApiError::conflict(
            "embedding_profile_changed",
            "The active embedding profile changed during this search.",
        ));
    }
    if profile.is_none() && matches!(first.try_get::<&str, _>("positive")?, "" | "T") {
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
        let mut retrieved_by = Vec::new();
        if row.try_get("by_keyword")? {
            retrieved_by.push(SearchMode::Keyword);
        }
        if row.try_get("by_vector")? {
            retrieved_by.push(SearchMode::Semantic);
        }
        data.push(SearchMatch {
            message_id: id,
            session_id: row.try_get("session_id")?,
            source_id: row.try_get("source_id")?,
            member: row.try_get("member")?,
            device_label: row.try_get("device_label")?,
            recorded_cwd: row.try_get("recorded_cwd")?,
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
            retrieved_by,
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
        mode: request.mode,
        coverage: SearchCoverage {
            keyword_ready_messages: first.try_get::<i64, _>("ready")? as u64,
            embedding_eligible_passages: first.try_get::<i64, _>("eligible_count")? as u64,
            embedded_passages: first.try_get::<i64, _>("embedded_count")? as u64,
            embedding_profile_id: profile.map(|p| p.id),
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
