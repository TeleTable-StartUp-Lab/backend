use crate::auth::models::Claims;
use crate::auth::roles;
use crate::auth::security::decode_jwt;
use crate::notifications::models::RobotNotification;
use crate::robot::models::{
    NodesResponse, QueuedRoute, RobotCommand, RobotStatusUpdate, RouteSelectionRequest,
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

use chrono::Utc;
use futures::{sink::SinkExt, stream::StreamExt};
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use tokio::time::{self, Duration, MissedTickBehavior};
use uuid::Uuid;

const ROBOT_CONTROL_HEARTBEAT_INTERVAL_SECS: u64 = 10;

async fn emit_runtime_warning(state: &Arc<AppState>, message: impl Into<String>) {
    let message = message.into();
    tracing::warn!("{message}");
    let _ = state
        .robot_state
        .notification_sender
        .send(RobotNotification {
            id: Uuid::new_v4(),
            priority: "WARN".to_string(),
            message,
            received_at: Utc::now(),
        });
}

pub async fn robot_control_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_robot_socket(socket, state))
}

async fn handle_robot_socket(socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.robot_state.command_sender.subscribe();
    let (mut sender, mut receiver) = socket.split();
    let mut heartbeat = time::interval(Duration::from_secs(ROBOT_CONTROL_HEARTBEAT_INTERVAL_SECS));
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);

    state.robot_state.register_control_channel_connection();
    crate::robot::broadcast_status_update(&state).await;

    loop {
        tokio::select! {
            command = rx.recv() => {
                match command {
                    Ok(cmd) => {
                        let msg = match serde_json::to_string(&cmd) {
                            Ok(msg) => msg,
                            Err(error) => {
                                tracing::error!(error = %error, "Failed to serialize robot command");
                                continue;
                            }
                        };

                        if sender.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "Robot control channel lagged behind command stream");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Ping(payload))) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => continue,
                    Some(Err(error)) => {
                        tracing::warn!(error = %error, "Robot control socket receive error");
                        break;
                    }
                    None => break,
                }
            }
            _ = heartbeat.tick() => {
                if sender.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
        }
    }

    state.robot_state.unregister_control_channel_connection();
    crate::robot::broadcast_status_update(&state).await;
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
        Err(e) => {
            tracing::warn!(error = %e, "WebSocket manual control - invalid token (401)");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    ws.on_upgrade(move |socket| handle_manual_socket(socket, state, claims))
}

pub async fn robot_events_ws(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let claims = match decode_jwt(&params.token, &state.config.jwt_secret) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "WebSocket robot events - invalid token (401)");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    if !roles::can_view(&claims.role) {
        tracing::warn!(
            user_id = %claims.sub,
            role    = %claims.role,
            "WebSocket robot events - insufficient permissions (403)"
        );
        return StatusCode::FORBIDDEN.into_response();
    }

    ws.on_upgrade(move |socket| handle_events_socket(socket, state))
}

async fn handle_events_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut status_rx = state.robot_state.status_sender.subscribe();
    let mut notification_rx = state.robot_state.notification_sender.subscribe();

    let initial_status = crate::robot::build_status_update(&state).await;
    let initial_status_event = WsStatusUpdateEvent {
        event: "status_update",
        data: initial_status,
    };
    if let Ok(msg) = serde_json::to_string(&initial_status_event) {
        if socket.send(Message::Text(msg.into())).await.is_err() {
            return;
        }
    }

    loop {
        tokio::select! {
            notification = notification_rx.recv() => {
                match notification {
                    Ok(notification) => {
                        let envelope = WsNotificationEvent {
                            event: "robot_notification",
                            data: notification,
                        };

                        if let Ok(msg) = serde_json::to_string(&envelope) {
                            if socket.send(Message::Text(msg.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            status_update = status_rx.recv() => {
                match status_update {
                    Ok(status_update) => {
                        let envelope = WsStatusUpdateEvent {
                            event: "status_update",
                            data: status_update,
                        };

                        if let Ok(msg) = serde_json::to_string(&envelope) {
                            if socket.send(Message::Text(msg.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
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
                // Viewers cannot send commands.
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

            if !state.robot_state.is_control_channel_connected() {
                emit_runtime_warning(
                    &state,
                    format!(
                        "Rejected manual command from {} because robot control channel is disconnected",
                        claims.name
                    ),
                )
                .await;
                continue;
            }

            // 2. Admin Preemption & Logic
            if is_admin {
                // Admin can do anything
                // Check if this is a navigation command that needs preemption
                let mut debug_changed = false;
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
                        debug_changed = true;
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
                        debug_changed = true;
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
                        debug_changed = true;
                    }
                }

                // Execute Admin Command
                if state.robot_state.command_sender.send(cmd).is_err() {
                    emit_runtime_warning(
                        &state,
                        "Failed to send admin command because robot control channel has no receivers",
                    )
                    .await;
                    break;
                }
                if debug_changed {
                    crate::robot::broadcast_status_update(&state).await;
                }
            } else if is_operator {
                // Operators cannot send navigation/cancel commands via WS
                if matches!(cmd, RobotCommand::Navigate { .. } | RobotCommand::Cancel) {
                    continue;
                }

                // Operator must hold a non-expired lock to send commands
                let lock = state.robot_state.manual_lock.read().await;
                let is_valid_holder = if let Some(l) = &*lock {
                    l.holder_id.to_string() == claims.sub && l.expires_at > chrono::Utc::now()
                } else {
                    false
                };

                if is_valid_holder {
                    if state.robot_state.command_sender.send(cmd).is_err() {
                        emit_runtime_warning(
                            &state,
                            "Failed to send manual drive command because robot control channel has no receivers",
                        )
                        .await;
                        break;
                    }
                }
            }
        }
    }
}

#[derive(Serialize)]
struct WsNotificationEvent {
    event: &'static str,
    data: RobotNotification,
}

#[derive(Serialize)]
struct WsStatusUpdateEvent {
    event: &'static str,
    data: RobotStatusUpdate,
}

pub async fn get_nodes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(NodesResponse {
            nodes: state.static_nodes.clone(),
        }),
    )
        .into_response()
}

pub async fn get_robot_debug(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let debug_snapshot = crate::robot::build_debug_snapshot(&state).await;
    (StatusCode::OK, Json(debug_snapshot)).into_response()
}

pub async fn select_route(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<RouteSelectionRequest>,
) -> impl IntoResponse {
    if !roles::can_operate(&claims.role) {
        tracing::warn!(
            user_id = %claims.sub,
            name    = %claims.name,
            role    = %claims.role,
            "Permission denied - select_route requires operator or above (403)"
        );
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
    crate::robot::broadcast_status_update(&state).await;

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
        tracing::warn!(
            user_id = %claims.sub,
            name    = %claims.name,
            role    = %claims.role,
            "Permission denied - acquire_lock requires operator or above (403)"
        );
        return StatusCode::FORBIDDEN.into_response();
    }

    let is_admin = roles::is_admin(&claims.role);

    if !state.robot_state.is_robot_connected().await {
        return Json(serde_json::json!({
            "status": "error",
            "message": "Cannot acquire lock because robot is not connected"
        }))
        .into_response();
    }

    if !state.robot_state.is_control_channel_connected() {
        return Json(serde_json::json!({
            "status": "error",
            "message": "Cannot acquire lock because robot control channel is not connected"
        }))
        .into_response();
    }

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

            tracing::info!("Admin {} revoked lock from {}", claims.name, l.holder_name);
        }
    }

    if let Ok(user_id) = Uuid::parse_str(&claims.sub) {
        *lock = Some(super::state::LockInfo {
            holder_id: user_id,
            holder_name: claims.name.clone(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
        });

        let message = if is_admin && state.robot_state.active_route.read().await.is_some() {
            "Admin lock acquired while automated route is active"
        } else {
            "Lock acquired"
        };

        tracing::info!(
            user_id = %user_id,
            name    = %claims.name,
            role    = %claims.role,
            "Manual drive lock acquired"
        );

        let response = Json(serde_json::json!({
            "status": "success",
            "message": message
        }))
        .into_response();

        drop(lock);
        let state_for_broadcast = state.clone();
        tokio::spawn(async move {
            crate::robot::broadcast_status_update(&state_for_broadcast).await;
        });

        response
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
        tracing::warn!(
            user_id = %claims.sub,
            name    = %claims.name,
            role    = %claims.role,
            "Permission denied - release_lock requires operator or above (403)"
        );
        return StatusCode::FORBIDDEN.into_response();
    }

    let mut lock = state.robot_state.manual_lock.write().await;

    // Only holder can release.
    if let Some(l) = &*lock {
        if l.holder_id.to_string() == claims.sub {
            tracing::info!(
                user_id = %claims.sub,
                name    = %claims.name,
                "Manual drive lock released"
            );
            *lock = None;
            let response = Json(serde_json::json!({
                "status": "success",
                "message": "Lock released"
            }))
            .into_response();

            drop(lock);
            let state_for_broadcast = state.clone();
            tokio::spawn(async move {
                crate::robot::broadcast_status_update(&state_for_broadcast).await;
            });

            return response;
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
    let robot_connected = state.robot_state.is_robot_connected().await;

    if let Some(url) = &*robot_url {
        if !robot_connected {
            return Json(serde_json::json!({
                "status": "error",
                "connected": false,
                "message": "Robot registered but no recent state updates (stale)",
                "url": url
            }));
        }

        match state.http_client.get(format!("{url}/health")).send().await {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    tracing::warn!(
                        endpoint    = %format!("{url}/health"),
                        status_code = status.as_u16(),
                        "External API failure - robot /health returned non-success status"
                    );
                }
                Json(serde_json::json!({
                    "status": "success",
                    "connected": true,
                    "robot_status": status.as_u16(),
                    "url": url
                }))
            }
            Err(e) => {
                tracing::error!(
                    endpoint = %format!("{url}/health"),
                    error    = %e,
                    "External API failure - could not reach robot /health"
                );
                Json(serde_json::json!({
                    "status": "error",
                    "connected": false,
                    "message": format!("Failed to reach robot: {}", e),
                    "url": url
                }))
            }
        }
    } else {
        Json(serde_json::json!({
            "status": "error",
            "connected": false,
            "message": "No robot URL registered"
        }))
    }
}
