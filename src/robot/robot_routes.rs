use crate::robot::models::{RobotEvent, RobotState};
use crate::AppState;
use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::net::SocketAddr;
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
    {
        let mut active_route_guard = state.robot_state.active_route.write().await;

        // Check if we just finished a route
        if active_route_guard.is_some() && payload.drive_mode == "IDLE" {
            // Assumption: IDLE means finished.
            *active_route_guard = None;
        }
    }

    // Trigger processing (checks IDLE, Lock, Queue)
    crate::robot::process_queue(&state).await;

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

#[derive(Deserialize)]
pub struct RobotRegistration {
    port: u16,
}

pub async fn register_robot(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<RobotRegistration>,
) -> impl IntoResponse {
    let mut ip = addr.ip();

    // Prioritize X-Real-IP, then X-Forwarded-For, then socket address
    if let Some(real_ip_str) = headers.get("X-Real-IP").and_then(|v| v.to_str().ok()) {
        if let Ok(parsed_ip) = real_ip_str.parse() {
            ip = parsed_ip;
        }
    } else if let Some(fwd_str) = headers.get("X-Forwarded-For").and_then(|v| v.to_str().ok()) {
        if let Some(first_ip) = fwd_str.split(',').next() {
            if let Ok(parsed_ip) = first_ip.trim().parse() {
                ip = parsed_ip;
            }
        }
    }

    let port = payload.port;
    let url = format!("http://{ip}:{port}");

    let mut url_lock = state.robot_state.robot_url.write().await;
    if url_lock.as_deref() != Some(&url) {
        tracing::info!("Registered robot at {}", url);
        *url_lock = Some(url);
    }

    StatusCode::OK
}
