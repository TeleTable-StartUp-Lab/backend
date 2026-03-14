use axum::{
    extract::{ConnectInfo, FromRequestParts, Path, Query, State},
    http::{request::Parts, HeaderMap, StatusCode},
    Json,
};
use redis::AsyncCommands;
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

const REGISTER_RATE_LIMIT_MAX_ATTEMPTS: u64 = 5;
const REGISTER_RATE_LIMIT_WINDOW_SECONDS: u64 = 600;
const REGISTER_SWIFTSHADER_TIMEOUT_SECONDS: u64 = 86_400;

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

fn contains_swiftshader_renderer(value: &serde_json::Value) -> bool {
    fn scan(value: &serde_json::Value, renderer_context: bool) -> bool {
        match value {
            serde_json::Value::Object(map) => map.iter().any(|(k, v)| {
                let is_renderer_key = k.to_ascii_lowercase().contains("renderer");
                scan(v, renderer_context || is_renderer_key)
            }),
            serde_json::Value::Array(items) => {
                items.iter().any(|item| scan(item, renderer_context))
            }
            serde_json::Value::String(s) => {
                renderer_context && s.to_ascii_lowercase().contains("swiftshader")
            }
            _ => false,
        }
    }

    scan(value, false)
}

async fn enforce_registration_rate_limit(
    state: &AppState,
    client_ip: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if client_ip == "unknown" {
        tracing::warn!("Skipping register rate limit because client IP is unknown");
        return Ok(());
    }

    let key = format!("ratelimit:register:ip:{client_ip}");
    let blocked_key = format!("ratelimit:register:blocked:ip:{client_ip}");
    let mut redis = state.redis.clone();

    match redis.exists::<_, bool>(&blocked_key).await {
        Ok(true) => {
            let ttl_seconds = redis.ttl(&blocked_key).await.unwrap_or(-1);
            tracing::warn!(
                ip = %client_ip,
                ttl_seconds,
                "Blocked signup attempt from timed-out IP"
            );
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "Account creation is temporarily blocked for this IP.",
                    "retry_after_seconds": if ttl_seconds > 0 { ttl_seconds } else { REGISTER_RATE_LIMIT_WINDOW_SECONDS as i64 }
                })),
            ));
        }
        Ok(false) => {}
        Err(e) => {
            tracing::error!(error = %e, ip = %client_ip, "Failed to check signup IP timeout state");
            // Fail open to avoid signup outage if Redis is temporarily unavailable.
            return Ok(());
        }
    }

    let attempt_count: u64 = match redis.incr(&key, 1_u64).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, ip = %client_ip, "Failed to apply register rate limit");
            // Fail open to avoid signup outage if Redis is temporarily unavailable.
            return Ok(());
        }
    };

    if attempt_count == 1 {
        let _: Result<bool, redis::RedisError> =
            redis.expire(&key, REGISTER_RATE_LIMIT_WINDOW_SECONDS as i64).await;
    }

    if attempt_count > REGISTER_RATE_LIMIT_MAX_ATTEMPTS {
        tracing::warn!(
            ip = %client_ip,
            attempt_count,
            max_attempts = REGISTER_RATE_LIMIT_MAX_ATTEMPTS,
            "Register rate limit exceeded"
        );
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "Too many account creation attempts from this IP. Please try again later.",
                "retry_after_seconds": REGISTER_RATE_LIMIT_WINDOW_SECONDS
            })),
        ));
    }

    Ok(())
}

async fn timeout_registration_ip(
    state: &AppState,
    client_ip: &str,
    timeout_seconds: u64,
    reason: &str,
) {
    if client_ip == "unknown" {
        tracing::warn!(reason = %reason, "Cannot timeout unknown IP");
        return;
    }

    let blocked_key = format!("ratelimit:register:blocked:ip:{client_ip}");
    let attempts_key = format!("ratelimit:register:ip:{client_ip}");
    let mut redis = state.redis.clone();

    if let Err(e) = redis
        .set_ex::<_, _, ()>(&blocked_key, reason, timeout_seconds)
        .await
    {
        tracing::error!(
            error = %e,
            ip = %client_ip,
            timeout_seconds,
            reason = %reason,
            "Failed to set signup IP timeout"
        );
        return;
    }

    let _: Result<u64, redis::RedisError> = redis.del(attempts_key).await;
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

    enforce_registration_rate_limit(&state, &client_ip).await?;

    if contains_swiftshader_renderer(&fingerprint_data) {
        timeout_registration_ip(
            &state,
            &client_ip,
            REGISTER_SWIFTSHADER_TIMEOUT_SECONDS,
            "swiftshader_renderer",
        )
        .await;

        tracing::warn!(
            ip = %client_ip,
            timeout_seconds = REGISTER_SWIFTSHADER_TIMEOUT_SECONDS,
            "Blocked signup due to SwiftShader fingerprint signal"
        );

        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "Account creation is temporarily blocked for this IP.",
                "retry_after_seconds": REGISTER_SWIFTSHADER_TIMEOUT_SECONDS
            })),
        ));
    }

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
        "INSERT INTO users (id, name, email, password_hash, role) VALUES ($1, $2, $3, $4, $5) RETURNING *",
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
        "INSERT INTO sessions (id, user_id, ip_address, fingerprint_data, user_agent) VALUES ($1, $2, $3, $4, $5)",
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

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
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

    sqlx::query(
        "INSERT INTO sessions (id, user_id, ip_address, fingerprint_data, user_agent) VALUES ($1, $2, $3, $4, $5)",
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
        let user = sqlx::query_as::<_, User>(
            r#"
            SELECT u.*, ls.last_sign_on
            FROM users u
            LEFT JOIN user_last_sign_on ls ON ls.user_id = u.id
            WHERE u.id = $1
            "#,
        )
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
        let user = sqlx::query_as::<_, User>(
            r#"
            SELECT u.*, ls.last_sign_on
            FROM users u
            LEFT JOIN user_last_sign_on ls ON ls.user_id = u.id
            WHERE u.id = $1
            "#,
        )
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
        let users = sqlx::query_as::<_, User>(
            r#"
            SELECT u.*, ls.last_sign_on
            FROM users u
            LEFT JOIN user_last_sign_on ls ON ls.user_id = u.id
            "#,
        )
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
    let users = sqlx::query_as::<_, User>(
        r#"
        SELECT u.*, ls.last_sign_on
        FROM users u
        LEFT JOIN user_last_sign_on ls ON ls.user_id = u.id
        ORDER BY u.created_at DESC
        "#,
    )
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
        "SELECT id, user_id, ip_address, fingerprint_data, user_agent, created_at FROM sessions WHERE user_id = $1 ORDER BY created_at DESC",
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
