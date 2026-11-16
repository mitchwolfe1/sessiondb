use super::*;
use crate::auth::{Principal, is_admin, organization_role};
use axum::Extension;
use sessiondb_types::{
    Invitation, InvitationCreate, ListOptions, Me, Membership, Organization, OrganizationRole, User,
};

pub(super) async fn login(
    State(state): State<Arc<AppState>>,
    Extension(principal): Extension<Principal>,
) -> ApiResult<Json<Me>> {
    let c = &principal.claims;
    let mut tx = state.pool.begin().await?;
    let email = if c.email_verified {
        c.email.as_deref()
    } else {
        None
    };
    let row = sqlx::query("INSERT INTO users(id,issuer,subject,display_name,email) VALUES($1,$2,$3,$4,$5) ON CONFLICT(issuer,subject) DO UPDATE SET display_name=EXCLUDED.display_name,email=EXCLUDED.email RETURNING id,display_name,email")
        .bind(Uuid::new_v4()).bind(&c.iss).bind(&c.sub).bind(c.name.as_deref().or(c.preferred_username.as_deref()).unwrap_or("SessionDB user")).bind(email).fetch_one(&mut *tx).await?;
    let user = User {
        id: row.get("id"),
        display_name: row.get("display_name"),
        email: row.get("email"),
    };
    if let Some(email) = email {
        let invitations = sqlx::query("SELECT id,organization_id,role FROM invitations WHERE lower(email)=lower($1) AND status='pending' AND expires_at>now() ORDER BY id FOR UPDATE").bind(email).fetch_all(&mut *tx).await?;
        for i in invitations {
            sqlx::query("INSERT INTO memberships(organization_id,user_id,role) VALUES($1,$2,$3) ON CONFLICT(organization_id,user_id) DO NOTHING").bind(i.get::<Uuid,_>("organization_id")).bind(user.id).bind(i.get::<String,_>("role")).execute(&mut *tx).await?;
            sqlx::query("UPDATE invitations SET status='accepted' WHERE id=$1")
                .bind(i.get::<Uuid, _>("id"))
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    Ok(Json(account(&state, user).await?))
}
async fn account(state: &AppState, user: User) -> ApiResult<Me> {
    let rows = sqlx::query("SELECT organization_id,role,status FROM memberships WHERE user_id=$1 AND status='active' ORDER BY organization_id").bind(user.id).fetch_all(&state.pool).await?;
    let memberships = rows
        .into_iter()
        .map(|r| {
            Ok(Membership {
                user: user.clone(),
                organization_id: r.get("organization_id"),
                role: r
                    .get::<String, _>("role")
                    .parse()
                    .map_err(ApiError::internal)?,
                status: r.get("status"),
            })
        })
        .collect::<ApiResult<Vec<_>>>()?;
    Ok(Me { user, memberships })
}
pub(super) async fn me(
    State(state): State<Arc<AppState>>,
    Extension(principal): Extension<Principal>,
) -> ApiResult<Json<Me>> {
    Ok(Json(
        account(&state, principal.user.ok_or_else(ApiError::unauthorized)?).await?,
    ))
}
pub(super) async fn organizations(
    State(state): State<Arc<AppState>>,
    Extension(principal): Extension<Principal>,
    ApiQuery(options): ApiQuery<ListOptions>,
) -> ApiResult<Json<Page<Organization>>> {
    if options.harness.is_some() {
        return Err(ApiError::bad(
            "unsupported_option",
            "harness is not supported here.",
        ));
    }
    let user_id = principal.user_id()?;
    let scope = scope(("organizations", user_id))?;
    let cursor = decode_cursor(options.cursor.as_deref(), &scope)?;
    let size = limit(options.limit)?;
    let rows = sqlx::query("SELECT o.id,o.slug,o.name FROM organizations o JOIN memberships m ON m.organization_id=o.id WHERE m.user_id=$1 AND m.status='active' AND ($2::uuid IS NULL OR o.id>$2) ORDER BY o.id LIMIT $3").bind(user_id).bind(cursor.map(|c| c.id)).bind(size as i64 + 1).fetch_all(&state.pool).await?;
    let data = rows
        .into_iter()
        .map(|r| Organization {
            id: r.get("id"),
            slug: r.get("slug"),
            name: r.get("name"),
        })
        .collect();
    Ok(Json(page(data, size, scope, |o| o.id)?))
}
pub(super) async fn invite(
    State(state): State<Arc<AppState>>,
    Extension(principal): Extension<Principal>,
    ApiPath(org): ApiPath<Uuid>,
    ApiJson(input): ApiJson<InvitationCreate>,
) -> ApiResult<Response> {
    let role = organization_role(&state.pool, org, &principal).await?;
    if !is_admin(role) {
        return Err(ApiError::forbidden());
    }
    if !matches!(input.role.as_str(), "admin" | "member")
        || (input.role == "admin" && role != OrganizationRole::Owner)
    {
        return Err(ApiError::forbidden());
    }
    if input.email.len() > 254
        || input.email.trim() != input.email
        || !input.email.contains('@')
        || input.email.chars().any(char::is_control)
    {
        return Err(ApiError::bad(
            "invalid_email",
            "A valid email address is required.",
        ));
    }
    let invitation = Invitation {
        id: Uuid::new_v4(),
        email: input.email.to_lowercase(),
        role: input.role,
        expires_at: Utc::now() + chrono::Duration::days(7),
        status: "pending".into(),
    };
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE invitations SET status='expired' WHERE organization_id=$1 AND lower(email)=$2 AND status='pending' AND expires_at<=now()").bind(org).bind(&invitation.email).execute(&mut *tx).await?;
    let result = sqlx::query("INSERT INTO invitations(id,organization_id,email,role,expires_at) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING").bind(invitation.id).bind(org).bind(&invitation.email).bind(&invitation.role).bind(invitation.expires_at).execute(&mut *tx).await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::conflict(
            "invitation_exists",
            "An active invitation already exists.",
        ));
    }
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(invitation)).into_response())
}
