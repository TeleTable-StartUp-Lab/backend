use crate::auth::models::Claims;
use crate::auth::roles;
use crate::auth::security::decode_jwt;
use crate::robot::models::{
    LastRoute, NodesResponse, QueuedRoute, RobotCommand, RouteSelectionRequest, StatusResponse,
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
use chrono::Utc;

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
    let role = claims.role.as_str();
    let is_admin = roles::is_admin(role);
    let is_operator = roles::is_operator(role);

    while let Some(Ok(msg)) = socket.next().await {
        if let Message::Text(text) = msg {
            let cmd: RobotCommand = match serde_json::from_str(&text) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // 1. Role Permission Check - Basic Level
            if !roles::can_operate(role) {
                // Viewers cannot send commands
                continue;
            }

            // 1a. Admin-only commands
            if !is_admin
                && matches!(
                    cmd,
                    RobotCommand::Led { .. }
                        | RobotCommand::AudioBeep { .. }
                        | RobotCommand::AudioVolume { .. }
                )
            {
                continue;
            }

            // 2. Admin Preemption & Logic
            if is_admin {
                // Admin can do anything
                // Check if this is a navigation command that needs preemption
                if let RobotCommand::Navigate { .. } = &cmd {
                    let mut lock = state.robot_state.manual_lock.write().await;
                    let should_revoke = if let Some(l) = &*lock {
                        l.holder_id.to_string() != claims.sub
                    } else {
                        false
                    };

                    if should_revoke {
                        let name = lock
                            .as_ref()
                            .map(|l| l.holder_name.clone())
                            .unwrap_or_default();
                        *lock = None; // Forcibly revoke
                        tracing::info!("Admin revoked lock from operator {}", name);
                    }
                    drop(lock);

                    // Handle Queue Preemption
                    // Cancel active route, move to front of queue
                    let mut active_route_guard = state.robot_state.active_route.write().await;
                    if let Some(active) = active_route_guard.take() {
                        // There was an active route. Cancel it on robot.
                        let _ = state.robot_state.command_sender.send(RobotCommand::Cancel);

                        // Move to front of queue
                        // "Resumed route starts from beginning" -> So we just put it back in queue with same Start/End
                        let mut queue = state.robot_state.queue.write().await;
                        queue.push_front(active);
                    }

                    // Track this WS navigation as the active route (so it appears in queue view)
                    if let RobotCommand::Navigate { start, destination } = &cmd {
                        *active_route_guard = Some(QueuedRoute {
                            id: Uuid::new_v4(),
                            start: start.clone(),
                            destination: destination.clone(),
                            added_at: Utc::now(),
                            added_by: claims.name.clone(),
                        });
                    }
                }

                // Execute Admin Command
                let _ = state.robot_state.command_sender.send(cmd);
            } else if is_operator {
                // Operators cannot send navigation/cancel commands via WS
                if matches!(cmd, RobotCommand::Navigate { .. } | RobotCommand::Cancel) {
                    continue;
                }

                // Operator Logic
                // Must hold lock to drive manually or send commands?
                // "acquire the manual mode lock"
                let lock = state.robot_state.manual_lock.read().await;
                let is_holder = if let Some(l) = &*lock {
                    l.holder_id.to_string() == claims.sub
                } else {
                    false
                };

                if is_holder {
                    // Verify allow-list if needed (we only have expected commands in enum)
                    let _ = state.robot_state.command_sender.send(cmd);
                }
            }
        }
    }
}

pub async fn get_nodes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Check Redis cache first
    let mut redis = state.redis.clone();
    if let Ok(Some(nodes)) = crate::cache::CacheService::get_nodes(&mut redis).await {
        return (
            StatusCode::OK,
            Json(NodesResponse { nodes }),
        )
            .into_response();
    }

    // Check in-memory cache
    if let Some(nodes) = &*state.robot_state.cached_nodes.read().await {
        // Update Redis cache
        let _ = crate::cache::CacheService::cache_nodes(&mut redis, nodes).await;
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
        match state.http_client.get(format!("{url}/nodes")).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    // Assume robot returns { "nodes": ["Node1", "Node2"] }
                    if let Ok(nodes_resp) = resp.json::<NodesResponse>().await {
                        // Cache it in both places
                        let mut cache = state.robot_state.cached_nodes.write().await;
                        *cache = Some(nodes_resp.nodes.clone());
                        let _ = crate::cache::CacheService::cache_nodes(&mut redis, &nodes_resp.nodes).await;

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
    Extension(claims): Extension<Claims>,
    Json(payload): Json<RouteSelectionRequest>,
) -> impl IntoResponse {
    if !roles::can_operate(&claims.role) {
        return StatusCode::FORBIDDEN.into_response();
    }

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

    // Add to Queue instead of direct send
    // This allows the queue view to see it, and process_queue to handle dispatch
    let route = QueuedRoute {
        id: Uuid::new_v4(),
        start: payload.start,
        destination: payload.destination,
        added_at: Utc::now(),
        added_by: claims.name,
    };

    {
        let mut queue = state.robot_state.queue.write().await;
        queue.push_back(route);
    }

    // Attempt dispatch
    crate::robot::process_queue(&state).await;

    Json(serde_json::json!({
        "status": "success",
        "message": "Route queued"
    }))
    .into_response()
}

pub async fn acquire_lock(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> impl IntoResponse {
    if !roles::can_operate(&claims.role) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let is_admin = roles::is_admin(&claims.role);

    // Check if queue is active
    if !is_admin && state.robot_state.active_route.read().await.is_some() {
        return Json(serde_json::json!({
            "status": "error",
            "message": "Cannot acquire lock while automated route is active"
        }))
        .into_response();
    }

    let mut lock = state.robot_state.manual_lock.write().await;

    if let Some(l) = &*lock {
        if l.expires_at > chrono::Utc::now() && l.holder_id.to_string() != claims.sub {
            if !is_admin {
                return Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Lock held by {}", l.holder_name)
                }))
                .into_response();
            }

            tracing::info!(
                "Admin {} revoked lock from {}",
                claims.name,
                l.holder_name
            );
        }
    }

    if let Ok(user_id) = Uuid::parse_str(&claims.sub) {
        *lock = Some(super::state::LockInfo {
            holder_id: user_id,
            holder_name: claims.name,
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
        });

        let message = if is_admin && state.robot_state.active_route.read().await.is_some() {
            "Admin lock acquired while automated route is active"
        } else {
            "Lock acquired"
        };

        Json(serde_json::json!({
            "status": "success",
            "message": message
        }))
        .into_response()
    } else {
        Json(serde_json::json!({
            "status": "error",
            "message": "Invalid User ID"
        }))
        .into_response()
    }
}

pub async fn release_lock(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> impl IntoResponse {
    if !roles::can_operate(&claims.role) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let mut lock = state.robot_state.manual_lock.write().await;

    // Only holder can release
    if let Some(l) = &*lock {
        if l.holder_id.to_string() == claims.sub {
            *lock = None;
            return Json(serde_json::json!({
                "status": "success",
                "message": "Lock released"
            }))
            .into_response();
        }
    }

    Json(serde_json::json!({
        "status": "error",
        "message": "You do not hold the lock"
    }))
    .into_response()
}

pub async fn check_robot_connection(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let robot_url = state.robot_state.robot_url.read().await;

    if let Some(url) = &*robot_url {
        match state.http_client.get(format!("{url}/health")).send().await {
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
