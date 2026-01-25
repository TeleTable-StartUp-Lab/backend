use crate::robot::models::{RobotCommand, RobotEvent, RobotState};
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

    // Update state
    {
        let mut current_state = state.robot_state.current_state.write().await;
        *current_state = Some(payload.clone());
    }

    // Queue Logic
    let mut active_route_guard = state.robot_state.active_route.write().await;

    // Check if we just finished a route
    if active_route_guard.is_some() && payload.drive_mode == "IDLE" {
        // Assumption: IDLE means finished.
        // What if it's "IDLE" but we haven't started moving yet?
        // We'll trust that if we sent a command, it transitions out of IDLE quickly, or we need a status 'MOVING'.
        // For this task, assuming transition to IDLE == Done.
        *active_route_guard = None;
    }

    // Process next item if idle
    if active_route_guard.is_none() && payload.drive_mode == "IDLE" {
        // Ensure robot is actually ready
        // Check if manual lock is held. If so, do not auto-drive.
        let lock = state.robot_state.manual_lock.read().await;
        if lock.is_none() {
            let mut queue = state.robot_state.queue.write().await;
            if let Some(next_route) = queue.pop_front() {
                // Send command
                let cmd = RobotCommand::Navigate {
                    start: next_route.start.clone(),
                    destination: next_route.destination.clone(),
                };

                // Set active
                *active_route_guard = Some(next_route);

                // Send
                let _ = state.robot_state.command_sender.send(cmd);
            }
        }
    }

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
