use super::models::{RobotCommand, RobotEvent, RobotState};
use crate::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
    Json,
};
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct LastRoute {
    pub start_node: String,
    pub end_node: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub system_health: String,
    pub battery_level: u8,
    pub drive_mode: String,
    pub cargo_status: String,
    pub last_route: Option<LastRoute>,
    pub position: String,
    pub manual_lock_holder_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NodesResponse {
    pub nodes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RouteSelectionRequest {
    pub start: String,
    pub destination: String,
}

pub async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let robot_state = state.robot_state.current_state.read().await;
    let lock_state = state.robot_state.manual_lock.read().await;

    let (system_health, battery_level, drive_mode, cargo_status, position, last_route) =
        if let Some(rs) = &*robot_state {
            (
                rs.system_health.clone(),
                rs.battery_level,
                rs.drive_mode.clone(),
                rs.cargo_status.clone(),
                rs.current_position.clone(),
                if let (Some(start), Some(end)) = (&rs.last_node, &rs.target_node) {
                    Some(LastRoute {
                        start_node: start.clone(),
                        end_node: end.clone(),
                    })
                } else {
                    None
                },
            )
        } else {
            (
                "UNKNOWN".to_string(),
                0,
                "UNKNOWN".to_string(),
                "UNKNOWN".to_string(),
                "UNKNOWN".to_string(),
                None,
            )
        };

    let manual_lock_holder_name = lock_state.as_ref().map(|l| l.holder_name.clone());

    let status = StatusResponse {
        system_health,
        battery_level,
        drive_mode,
        cargo_status,
        last_route,
        position,
        manual_lock_holder_name,
    };
    Json(status)
}

use axum::http::{HeaderMap, StatusCode};

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

pub async fn robot_control_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_robot_socket(socket, state))
}

async fn handle_robot_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.robot_state.command_sender.subscribe();

    while let Ok(cmd) = rx.recv().await {
        if let Ok(msg) = serde_json::to_string(&cmd) {
            if socket.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    }
}

pub async fn manual_control_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // TODO: Check if user holds the lock
    ws.on_upgrade(|socket| handle_manual_socket(socket, state))
}

async fn handle_manual_socket(mut socket: WebSocket, state: Arc<AppState>) {
    // Relay commands from user to robot
    while let Some(Ok(msg)) = socket.next().await {
        if let Message::Text(text) = msg {
            if let Ok(cmd) = serde_json::from_str::<RobotCommand>(&text) {
                let _ = state.robot_state.command_sender.send(cmd);
            }
        }
    }
}

pub async fn get_nodes() -> impl IntoResponse {
    // TODO: Fetch actual nodes configuration
    let nodes = NodesResponse {
        nodes: vec![
            "Mensa".to_string(),
            "Zimmer 101".to_string(),
            "Zimmer 102".to_string(),
        ],
    };
    Json(nodes)
}

pub async fn select_route(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RouteSelectionRequest>,
) -> impl IntoResponse {
    let cmd = RobotCommand::Navigate {
        start: payload.start,
        destination: payload.destination,
    };

    let _ = state.robot_state.command_sender.send(cmd);

    Json(serde_json::json!({
        "status": "success",
        "message": "Route selected"
    }))
}

pub async fn acquire_lock(
    State(state): State<Arc<AppState>>,
    // TODO: Extract user from auth header
) -> impl IntoResponse {
    // TODO: Implement locking logic with actual user
    let mut lock = state.robot_state.manual_lock.write().await;

    if lock.is_none() {
        *lock = Some(super::state::LockInfo {
            holder_id: Uuid::new_v4(), // Placeholder
            holder_name: "Test User".to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
        });

        Json(serde_json::json!({
            "status": "success",
            "message": "Lock acquired"
        }))
    } else {
        Json(serde_json::json!({
            "status": "error",
            "message": "Lock already held"
        }))
    }
}

pub async fn release_lock(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut lock = state.robot_state.manual_lock.write().await;
    *lock = None;

    Json(serde_json::json!({
        "status": "success",
        "message": "Lock released"
    }))
}
