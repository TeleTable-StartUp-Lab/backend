use crate::auth::auth::decode_jwt;
use crate::auth::models::Claims;
use crate::robot::models::{
    LastRoute, NodesResponse, RobotCommand, RouteSelectionRequest, StatusResponse,
};
use crate::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};

use futures::stream::StreamExt;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

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

#[derive(Deserialize)]
pub struct WsParams {
    token: String,
}

pub async fn manual_control_ws(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let claims = match decode_jwt(&params.token, &state.config.jwt_secret) {
        Ok(c) => c,
        Err(_) => return StatusCode::UNAUTHORIZED.into_response(),
    };

    ws.on_upgrade(move |socket| handle_manual_socket(socket, state, claims))
}

async fn handle_manual_socket(mut socket: WebSocket, state: Arc<AppState>, claims: Claims) {
    // Relay commands from user to robot
    // Verify lock ownership periodically or on action?
    // Doing it on action is safer.

    while let Some(Ok(msg)) = socket.next().await {
        if let Message::Text(text) = msg {
            // Re-check lock
            let lock = state.robot_state.manual_lock.read().await;
            let is_holder = if let Some(l) = &*lock {
                l.holder_id.to_string() == claims.sub
            } else {
                false
            };
            drop(lock); // Release read lock

            if !is_holder {
                // Ignore command or send error?
                // Websocket usually just ignores if unauthorized actions occur or closes.
                continue;
            }

            if let Ok(cmd) = serde_json::from_str::<RobotCommand>(&text) {
                let _ = state.robot_state.command_sender.send(cmd);
            }
        }
    }
}

pub async fn get_nodes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Check cache first
    if let Some(nodes) = &*state.robot_state.cached_nodes.read().await {
        return (
            StatusCode::OK,
            Json(NodesResponse {
                nodes: nodes.clone(),
            }),
        )
            .into_response();
    }

    // Attempt to fetch from robot
    let robot_url = state.robot_state.robot_url.read().await;
    if let Some(url) = &*robot_url {
        let client = reqwest::Client::new();
        match client.get(format!("{url}/nodes")).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    // Assume robot returns { "nodes": ["Node1", "Node2"] }
                    if let Ok(nodes_resp) = resp.json::<NodesResponse>().await {
                        // Cache it
                        let mut cache = state.robot_state.cached_nodes.write().await;
                        *cache = Some(nodes_resp.nodes.clone());

                        return (StatusCode::OK, Json(nodes_resp)).into_response();
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to fetch nodes from robot: {}", e);
            }
        }
    }

    // Fallback if no robot or fetch failed
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(NodesResponse { nodes: vec![] }),
    )
        .into_response()
}

pub async fn select_route(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RouteSelectionRequest>,
) -> impl IntoResponse {
    // Should route selection be locked? Maybe not, but concurrent nav commands are bad.
    // For now allow it broadly or require lock? Let's assume shared control allowed for nav unless locked?
    // User requested "correctly". If someone has manual lock, nav should be blocked?

    let lock = state.robot_state.manual_lock.read().await;
    if let Some(l) = &*lock {
        if l.expires_at > chrono::Utc::now() {
            return Json(serde_json::json!({
                "status": "error",
                "message": "Robot is manually locked"
            }))
            .into_response();
        }
    }

    let cmd = RobotCommand::Navigate {
        start: payload.start,
        destination: payload.destination,
    };

    let _ = state.robot_state.command_sender.send(cmd);

    Json(serde_json::json!({
        "status": "success",
        "message": "Route selected"
    }))
    .into_response()
}

pub async fn acquire_lock(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> impl IntoResponse {
    let mut lock = state.robot_state.manual_lock.write().await;

    if let Some(l) = &*lock {
        if l.expires_at > chrono::Utc::now() && l.holder_id.to_string() != claims.sub {
            return Json(serde_json::json!({
                "status": "error",
                "message": format!("Lock held by {}", l.holder_name)
            }));
        }
    }

    if let Ok(user_id) = Uuid::parse_str(&claims.sub) {
        *lock = Some(super::state::LockInfo {
            holder_id: user_id,
            holder_name: claims.name,
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
        });

        Json(serde_json::json!({
            "status": "success",
            "message": "Lock acquired"
        }))
    } else {
        Json(serde_json::json!({
            "status": "error",
            "message": "Invalid User ID"
        }))
    }
}

pub async fn release_lock(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> impl IntoResponse {
    let mut lock = state.robot_state.manual_lock.write().await;

    // Only holder can release
    if let Some(l) = &*lock {
        if l.holder_id.to_string() == claims.sub {
            *lock = None;
            return Json(serde_json::json!({
                "status": "success",
                "message": "Lock released"
            }));
        }
    }

    Json(serde_json::json!({
        "status": "error",
        "message": "You do not hold the lock"
    }))
}

pub async fn check_robot_connection(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let robot_url = state.robot_state.robot_url.read().await;

    if let Some(url) = &*robot_url {
        let client = reqwest::Client::new();
        match client.get(format!("{url}/health")).send().await {
            Ok(resp) => {
                let status = resp.status();
                Json(serde_json::json!({
                    "status": "success",
                    "robot_status": status.as_u16(),
                    "url": url
                }))
            }
            Err(e) => Json(serde_json::json!({
                "status": "error",
                "message": format!("Failed to reach robot: {}", e),
                "url": url
            })),
        }
    } else {
        Json(serde_json::json!({
            "status": "error",
            "message": "No robot URL registered"
        }))
    }
}
