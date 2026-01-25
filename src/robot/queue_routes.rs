use crate::AppState;
use crate::auth::models::Claims;
use crate::auth::roles;
use crate::robot::models::QueuedRoute;
use axum::{
    extract::{State, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
    Extension,
};
use std::sync::Arc;
use uuid::Uuid;
use chrono::Utc;
use serde::Deserialize;

pub async fn get_routes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let queue = state.robot_state.queue.read().await;
    Json(queue.clone())
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
    Json(serde_json::json!({
        "status": "success",
        "message": "Optimization triggered"
    })).into_response()
}
