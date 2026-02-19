use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::{extractor::AuthenticatedUser, roles},
    diary::models::{
        CreateDiaryRequest, DeleteDiaryRequest, DiaryEntry, DiaryEntryWithUser, DiaryQuery,
        DiaryResponse, DiaryResponseWithUser,
    },
    AppState,
};

pub async fn create_or_update_diary(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Json(payload): Json<CreateDiaryRequest>,
) -> Result<(StatusCode, Json<DiaryResponse>), (StatusCode, Json<serde_json::Value>)> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid user ID"})),
        )
    })?;

    // Trust JWT claims for role check (already validated by middleware)
    if !roles::can_operate(&claims.role) {
        tracing::warn!(
            user_id = %user_id,
            role    = %claims.role,
            "Permission denied - diary write requires operator or above (403)"
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Insufficient permissions" })),
        ));
    }

    let entry = if let Some(id) = payload.id {
        sqlx::query_as::<_, DiaryEntry>(
            r#"
            UPDATE diary_entries
            SET working_minutes = $1,
                text = $2
            WHERE id = $3 AND owner = $4
            RETURNING *
            "#,
        )
        .bind(payload.working_minutes)
        .bind(&payload.text)
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(
                query   = "UPDATE diary_entries SET ... WHERE id = ? AND owner = ?",
                error   = %e,
                user_id = %user_id,
                "DB error updating diary entry"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Diary entry not found" })),
        ))?
    } else {
        sqlx::query_as::<_, DiaryEntry>(
            r#"
            INSERT INTO diary_entries (id, owner, working_minutes, text)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(payload.working_minutes)
        .bind(&payload.text)
        .fetch_one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(
                query   = "INSERT INTO diary_entries ... RETURNING *",
                error   = %e,
                user_id = %user_id,
                "DB error creating diary entry"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?
    };
    // Invalidate diary cache
    let mut redis = state.redis.clone();
    let _ = crate::cache::CacheService::invalidate_diary(&mut redis, &entry.id.to_string()).await;
    Ok((
        if payload.id.is_some() {
            StatusCode::OK
        } else {
            StatusCode::CREATED
        },
        Json(entry.into()),
    ))
}

pub async fn get_diary(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Query(query): Query<DiaryQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid user ID"})),
        )
    })?;

    if let Some(id) = query.id {
        let entry = sqlx::query_as::<_, DiaryEntry>(
            "SELECT * FROM diary_entries WHERE id = $1 AND owner = $2",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Diary entry not found"})),
            )
        })?;

        Ok(Json(serde_json::json!(DiaryResponse::from(entry))))
    } else {
        let entries = sqlx::query_as::<_, DiaryEntry>(
            "SELECT * FROM diary_entries WHERE owner = $1 ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
        })?;

        let diary_responses: Vec<DiaryResponse> = entries.into_iter().map(|e| e.into()).collect();
        Ok(Json(serde_json::json!(diary_responses)))
    }
}

pub async fn get_all_diaries(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let entries = sqlx::query_as::<_, DiaryEntryWithUser>(
        r#"
        SELECT 
            d.id, 
            u.name AS owner, 
            d.working_minutes, 
            d.text, 
            d.created_at, 
            d.updated_at
        FROM diary_entries d
        INNER JOIN users u ON d.owner = u.id
        ORDER BY d.created_at DESC
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Database error: {}", e)})),
        )
    })?;

    // Map the results to your response DTO
    let response: Vec<DiaryResponseWithUser> = entries
        .into_iter()
        .map(DiaryResponseWithUser::from)
        .collect();

    Ok(Json(serde_json::json!(response)))
}

pub async fn delete_diary(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Json(payload): Json<DeleteDiaryRequest>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid user ID"})),
        )
    })?;

    // Trust JWT claims for role check (already validated by middleware)
    if !roles::can_operate(&claims.role) {
        tracing::warn!(
            user_id = %user_id,
            role    = %claims.role,
            "Permission denied - diary delete requires operator or above (403)"
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Insufficient permissions"})),
        ));
    }

    let result = sqlx::query("DELETE FROM diary_entries WHERE id = $1 AND owner = $2")
        .bind(payload.id)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(
                query    = "DELETE FROM diary_entries WHERE id = ? AND owner = ?",
                error    = %e,
                user_id  = %user_id,
                entry_id = %payload.id,
                "DB error deleting diary entry"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Database error: {}", e)})),
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Diary entry not found"})),
        ));
    }

    // Invalidate diary cache
    let mut redis = state.redis.clone();
    let _ = crate::cache::CacheService::invalidate_diary(&mut redis, &payload.id.to_string()).await;

    Ok(StatusCode::NO_CONTENT)
}
