use crate::auth::models::Claims;
use crate::auth::roles;
use crate::robot::models::QueuedRoute;
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub async fn get_routes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let active = state.robot_state.active_route.read().await.clone();
    let queue = state.robot_state.queue.read().await;
    let mut routes = Vec::with_capacity(queue.len() + if active.is_some() { 1 } else { 0 });

    if let Some(route) = active {
        routes.push(route);
    }

    routes.extend(queue.iter().cloned());

    Json(routes)
}

#[derive(Deserialize)]
pub struct AddRouteRequest {
    pub start: String,
    pub destination: String,
}

pub async fn add_route(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<AddRouteRequest>,
) -> impl IntoResponse {
    if !roles::is_admin(&claims.role) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let route = QueuedRoute {
        id: Uuid::new_v4(),
        start: payload.start,
        destination: payload.destination,
        added_at: Utc::now(),
        added_by: claims.name,
    };

    let mut queue = state.robot_state.queue.write().await;
    queue.push_back(route.clone());
    drop(queue);

    // Trigger queue processing
    crate::robot::process_queue(&state).await;

    (StatusCode::CREATED, Json(route)).into_response()
}

pub async fn delete_route(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if !roles::is_admin(&claims.role) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let mut queue = state.robot_state.queue.write().await;
    if let Some(pos) = queue.iter().position(|r| r.id == id) {
        queue.remove(pos);
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

pub async fn optimize_routes(
    State(_state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> impl IntoResponse {
    if !roles::is_admin(&claims.role) {
        return StatusCode::FORBIDDEN.into_response();
    }

    // Placeholder for optimization logic
    // TODO

    Json(serde_json::json!({
        "status": "success",
        "message": "Optimization triggered"
    }))
    .into_response()
}
