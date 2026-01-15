use crate::robot::models::{RobotEvent, RobotState};
use crate::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};

use std::sync::Arc;

pub async fn update_robot_state(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RobotState>,
) -> impl IntoResponse {
    let api_key = headers.get("X-Api-Key").and_then(|v| v.to_str().ok());

    if api_key != Some(&state.config.robot_api_key) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "status": "error",
                "message": "Invalid API Key"
            })),
        )
            .into_response();
    }

    let mut current_state = state.robot_state.current_state.write().await;
    *current_state = Some(payload);

    Json(serde_json::json!({
        "status": "success"
    }))
    .into_response()
}

pub async fn handle_robot_event(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RobotEvent>,
) -> impl IntoResponse {
    let api_key = headers.get("X-Api-Key").and_then(|v| v.to_str().ok());

    if api_key != Some(&state.config.robot_api_key) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "status": "error",
                "message": "Invalid API Key"
            })),
        )
            .into_response();
    }

    tracing::info!("Received robot event: {:?}", payload);
    // TODO: Handle specific events (e.g. notify users)

    Json(serde_json::json!({
        "status": "success"
    }))
    .into_response()
}
