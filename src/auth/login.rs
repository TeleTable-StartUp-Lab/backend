use axum::{
    extract::{ConnectInfo, FromRequestParts, Path, Query, State},
    http::{request::Parts, HeaderMap, StatusCode},
    Json,
};
use std::convert::Infallible;
use std::future::{ready, Future};
use std::net::SocketAddr;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::{
    extractor::AuthenticatedUser,
    models::{
        DeleteUserRequest, LoginRequest, LoginResponse, RegisterRequest, Session,
        UpdateUserRequest, User, UserQuery, UserResponse,
    },
    roles,
    security::{create_jwt, hash_password, verify_password},
};
use crate::AppState;

pub struct MaybeConnectInfo(pub Option<SocketAddr>);

impl<S> FromRequestParts<S> for MaybeConnectInfo
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let addr = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| *addr);
        ready(Ok(MaybeConnectInfo(addr)))
    }
}

/// Extract the real client IP from proxy-forwarded headers.
/// Falls back to the socket address (ConnectInfo), then returns "unknown".
fn extract_client_ip(headers: &HeaderMap, client_addr: Option<SocketAddr>) -> String {
    if let Some(ip) = headers.get("X-Real-IP").and_then(|v| v.to_str().ok()) {
        return ip.to_string();
    }
    if let Some(fwd) = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(first) = fwd.split(',').next() {
            return first.trim().to_string();
        }
    }
    if let Some(addr) = client_addr {
        return addr.ip().to_string();
    }
    "unknown".to_string()
}

fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get("User-Agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

pub async fn register(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    MaybeConnectInfo(client_addr): MaybeConnectInfo,
    Json(payload): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<UserResponse>), (StatusCode, Json<serde_json::Value>)> {
    let client_ip = extract_client_ip(&headers, client_addr);
    let user_agent = extract_user_agent(&headers);
    let fingerprint_data = payload
        .fingerprint_data
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));

    // Validate required fields before hitting the DB.
    if payload.email.trim().is_empty() || payload.name.trim().is_empty() {
        tracing::warn!(
            ip = %client_ip,
            "Registration validation failed - empty email or name"
        );
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Email and name are required"})),
        ));
    }

    let existing_user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(&payload.email)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(
                query   = "SELECT * FROM users WHERE email = ?",
                error   = %e,
                ip      = %client_ip,
                "DB error during registration email check"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
        })?;

    if existing_user.is_some() {
        tracing::warn!(
            email = %payload.email,
            ip    = %client_ip,
            "Registration failed - email already exists"
        );
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "User with this email already exists"})),
        ));
    }

    let password_hash = hash_password(&payload.password).await.map_err(|e| {
        tracing::error!(error = %e, "Password hashing failed during registration");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Password hashing error: {}", e)})),
        )
    })?;

    let mut tx = state.db.begin().await.map_err(|e| {
        tracing::error!(error = %e, "Failed to begin register transaction");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Database error: {}", e)})),
        )
    })?;

    let user_id = Uuid::new_v4();
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, name, email, password_hash, role, last_sign_on) VALUES ($1, $2, $3, $4, $5, NOW()) RETURNING *",
    )
    .bind(user_id)
    .bind(&payload.name)
    .bind(&payload.email)
    .bind(&password_hash)
    .bind(roles::VIEWER)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(
            query = "INSERT INTO users ... RETURNING *",
            error = %e,
            email = %payload.email,
            "DB error while creating user"
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to create user: {}", e)})),
        )
    })?;

    sqlx::query(
        "INSERT INTO sessions (id, user_id, ip_address, fingerprint_data, user_agent, is_current) VALUES ($1, $2, $3, $4, $5, TRUE)",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(&client_ip)
    .bind(&fingerprint_data)
    .bind(&user_agent)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(
            error = %e,
            user_id = %user_id,
            "Failed to insert initial user session"
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to create session: {}", e)})),
        )
    })?;

    tx.commit().await.map_err(|e| {
        tracing::error!(error = %e, user_id = %user_id, "Failed to commit register transaction");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Database error: {}", e)})),
        )
    })?;

    tracing::info!(
        user_id = %user.id,
        name    = %user.name,
        email   = %user.email,
        role    = %user.role,
        ip      = %client_ip,
        "New user registered"
    );

    Ok((StatusCode::CREATED, Json(user.into())))
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    MaybeConnectInfo(client_addr): MaybeConnectInfo,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<serde_json::Value>)> {
    let client_ip = extract_client_ip(&headers, client_addr);
    let user_agent = extract_user_agent(&headers);
    let fingerprint_data = payload
        .fingerprint_data
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));

    let mut user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(&payload.email)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(
                query = "SELECT * FROM users WHERE email = ?",
                error = %e,
                "DB error during login lookup"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
        })?
        .ok_or_else(|| {
            tracing::warn!(
                email             = %payload.email,
                ip                = %client_ip,
                attempted_password = %payload.password,
                "Failed login attempt - user not found"
            );
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Invalid credentials"})),
            )
        })?;

    let valid = verify_password(&payload.password, &user.password_hash)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %user.id, "Password verification error");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Password verification error: {}", e)})),
            )
        })?;

    if !valid {
        tracing::warn!(
            user_id           = %user.id,
            name              = %user.name,
            email             = %payload.email,
            ip                = %client_ip,
            attempted_password = %payload.password,
            "Failed login attempt - wrong password"
        );
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid credentials"})),
        ));
    }

    let mut tx = state.db.begin().await.map_err(|e| {
        tracing::error!(error = %e, "Failed to begin login transaction");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Database error: {}", e)})),
        )
    })?;

    sqlx::query("UPDATE users SET last_sign_on = NOW() WHERE id = $1")
        .bind(user.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %user.id, "Failed to update last_sign_on");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
        })?;

    sqlx::query("UPDATE sessions SET is_current = FALSE WHERE user_id = $1")
        .bind(user.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %user.id, "Failed to clear current sessions");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
        })?;

    sqlx::query(
        "INSERT INTO sessions (id, user_id, ip_address, fingerprint_data, user_agent, is_current) VALUES ($1, $2, $3, $4, $5, TRUE)",
    )
    .bind(Uuid::new_v4())
    .bind(user.id)
    .bind(&client_ip)
    .bind(&fingerprint_data)
    .bind(&user_agent)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, user_id = %user.id, "Failed to insert login session");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Database error: {}", e)})),
        )
    })?;

    tx.commit().await.map_err(|e| {
        tracing::error!(error = %e, user_id = %user.id, "Failed to commit login transaction");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Database error: {}", e)})),
        )
    })?;

    user.last_sign_on = Some(chrono::Utc::now());

    let token = create_jwt(
        &user.id.to_string(),
        &user.name,
        &user.role,
        &state.config.jwt_secret,
        state.config.jwt_expiry_hours,
    )
    .map_err(|e| {
        tracing::error!(error = %e, user_id = %user.id, "JWT generation failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Token generation error: {}", e)})),
        )
    })?;

    tracing::info!(
        user_id = %user.id,
        name    = %user.name,
        role    = %user.role,
        ip      = %client_ip,
        "Successful login"
    );

    // Cache user data for faster subsequent requests.
    let mut redis = state.redis.clone();
    let _ = crate::cache::CacheService::cache_user(&mut redis, &user.id.to_string(), &user).await;

    Ok(Json(LoginResponse { token }))
}

pub async fn get_me(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser(claims): AuthenticatedUser,
) -> Result<Json<UserResponse>, (StatusCode, Json<serde_json::Value>)> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        tracing::warn!(sub = %claims.sub, "get_me - invalid user ID in token");
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid user ID"})),
        )
    })?;

    // Try cache first.
    let mut redis = state.redis.clone();
    let user = if let Ok(Some(cached_user)) =
        crate::cache::CacheService::get_user::<User>(&mut redis, &user_id.to_string()).await
    {
        cached_user
    } else {
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await
            .map_err(|e| {
                tracing::error!(
                    query   = "SELECT * FROM users WHERE id = ?",
                    error   = %e,
                    user_id = %user_id,
                    "DB error in get_me"
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("Database error: {}", e)})),
                )
            })?;

        // Cache for next time.
        let _ =
            crate::cache::CacheService::cache_user(&mut redis, &user_id.to_string(), &user).await;
        user
    };

    Ok(Json(user.into()))
}

pub async fn get_user(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Some(id) = query.id {
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| {
                tracing::error!(
                    query   = "SELECT * FROM users WHERE id = ?",
                    error   = %e,
                    user_id = %id,
                    "DB error in get_user"
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("Database error: {}", e)})),
                )
            })?
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "User not found"})),
                )
            })?;

        Ok(Json(serde_json::json!(UserResponse::from(user))))
    } else {
        let users = sqlx::query_as::<_, User>("SELECT * FROM users")
            .fetch_all(&state.db)
            .await
            .map_err(|e| {
                tracing::error!(
                    query = "SELECT * FROM users",
                    error = %e,
                    "DB error listing all users"
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("Database error: {}", e)})),
                )
            })?;

        let user_responses: Vec<UserResponse> = users.into_iter().map(|u| u.into()).collect();
        Ok(Json(serde_json::json!(user_responses)))
    }
}

pub async fn get_users(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<UserResponse>>, (StatusCode, Json<serde_json::Value>)> {
    let users = sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(
                query = "SELECT * FROM users ORDER BY created_at DESC",
                error = %e,
                "DB error listing users"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
        })?;

    Ok(Json(users.into_iter().map(UserResponse::from).collect()))
}

pub async fn get_user_sessions(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<Vec<Session>>, (StatusCode, Json<serde_json::Value>)> {
    let user_exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(1) FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(
                query = "SELECT COUNT(1) FROM users WHERE id = ?",
                error = %e,
                user_id = %user_id,
                "DB error checking user for session history"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
        })?;

    if user_exists == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "User not found"})),
        ));
    }

    let sessions = sqlx::query_as::<_, Session>(
        "SELECT id, user_id, ip_address, fingerprint_data, user_agent, created_at, is_current FROM sessions WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(
            query = "SELECT ... FROM sessions WHERE user_id = ? ORDER BY created_at DESC",
            error = %e,
            user_id = %user_id,
            "DB error fetching session history"
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Database error: {}", e)})),
        )
    })?;

    Ok(Json(sessions))
}

pub async fn update_user(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, (StatusCode, Json<serde_json::Value>)> {
    let mut user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(payload.id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(
                query   = "SELECT * FROM users WHERE id = ?",
                error   = %e,
                user_id = %payload.id,
                "DB error fetching user for update"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "User not found"})),
            )
        })?;

    if let Some(name) = payload.name {
        user.name = name;
    }
    if let Some(email) = payload.email {
        user.email = email;
    }
    if let Some(ref role) = payload.role {
        tracing::info!(
            user_id  = %payload.id,
            old_role = %user.role,
            new_role = %role,
            "User role updated"
        );
        user.role = role.clone();
    }

    if let Some(password) = payload.password {
        if password.trim().is_empty() {
            tracing::warn!(user_id = %payload.id, "Update rejected - empty password provided");
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Password cannot be empty"})),
            ));
        }

        user.password_hash = hash_password(&password).await.map_err(|e| {
            tracing::error!(error = %e, user_id = %payload.id, "Password hashing failed during update");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Password hashing error: {}", e),
                })),
            )
        })?;
    }

    let updated_user = sqlx::query_as::<_, User>(
        "UPDATE users SET name = $1, email = $2, role = $3, password_hash = $4 WHERE id = $5 RETURNING *",
    )
    .bind(&user.name)
    .bind(&user.email)
    .bind(&user.role)
    .bind(&user.password_hash)
    .bind(payload.id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(
            query   = "UPDATE users SET ... WHERE id = ?",
            error   = %e,
            user_id = %payload.id,
            "DB error updating user"
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to update user: {}", e)})),
        )
    })?;

    tracing::info!(
        user_id = %payload.id,
        name    = %updated_user.name,
        email   = %updated_user.email,
        "User updated"
    );

    // Invalidate user cache and all JWT caches for this user after update.
    let mut redis = state.redis.clone();
    let _ =
        crate::cache::CacheService::invalidate_user(&mut redis, &payload.id.to_string()).await;
    let _ =
        crate::cache::CacheService::invalidate_user_jwts(&mut redis, &payload.id.to_string())
            .await;

    Ok(Json(updated_user.into()))
}

pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DeleteUserRequest>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(payload.id)
        .execute(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(
                query   = "DELETE FROM users WHERE id = ?",
                error   = %e,
                user_id = %payload.id,
                "DB error deleting user"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "User not found"})),
        ));
    }

    tracing::info!(user_id = %payload.id, "User deleted");

    Ok(StatusCode::NO_CONTENT)
}
