use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;

use crate::{
    auth::{extractor::AuthenticatedUser, roles},
    notifications::models::{NotificationHistoryQuery, RobotNotification},
    AppState,
};

pub async fn get_notification_history(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser(claims): AuthenticatedUser,
    Query(query): Query<NotificationHistoryQuery>,
) -> Result<Json<Vec<RobotNotification>>, (StatusCode, Json<serde_json::Value>)> {
    if !roles::can_view(&claims.role) {
        tracing::warn!(
            user_id = %claims.sub,
            name    = %claims.name,
            role    = %claims.role,
            "Permission denied - notification history requires viewer role or above (403)"
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Insufficient permissions" })),
        ));
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let offset = query.offset.unwrap_or(0).max(0);

    let notifications = sqlx::query_as::<_, RobotNotification>(
        r#"
        SELECT id, priority, message, received_at
        FROM robot_notifications
        ORDER BY received_at DESC
        LIMIT $1 OFFSET $2
        "#,
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "DB error fetching robot notification history");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to fetch notification history" })),
        )
    })?;

    Ok(Json(notifications))
}
