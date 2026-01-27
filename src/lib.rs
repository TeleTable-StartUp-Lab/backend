pub mod auth;
pub mod config;
pub mod database;
pub mod diary;
pub mod robot;

use crate::auth::security::{admin_middleware, auth_middleware};
use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};
pub use config::Config;
pub use database::{create_pool, create_redis_client};
use redis::aio::ConnectionManager;
pub use robot::state::SharedRobotState;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub redis: ConnectionManager,
    pub config: Config,
    pub robot_state: SharedRobotState,
}

pub fn create_router(state: Arc<AppState>) -> Router {
    // public routes (no authentication required)
    let public_routes = Router::new()
        .route("/", get(root))
        .route("/register", post(auth::login::register))
        .route("/login", post(auth::login::login))
        .route("/diary/all", get(diary::handlers::get_all_diaries));

    // protected routes (authentication required)
    let protected_routes = Router::new()
        .route("/me", get(auth::login::get_me))
        .route("/diary", post(diary::handlers::create_or_update_diary))
        .route("/diary", get(diary::handlers::get_diary))
        .route("/diary", delete(diary::handlers::delete_diary))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // admin routes (authentication + admin role required)
    let admin_routes = Router::new()
        .route("/user", get(auth::login::get_user))
        .route("/user", post(auth::login::update_user))
        .route("/user", delete(auth::login::delete_user))
        .route_layer(middleware::from_fn(admin_middleware))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // robot api routes (called by robot or public status)
    let robot_api_routes = Router::new()
        .route("/status", get(robot::client_routes::get_status))
        .route(
            "/table/state",
            post(robot::robot_routes::update_robot_state),
        )
        .route(
            "/table/event",
            post(robot::robot_routes::handle_robot_event),
        )
        .route(
            "/table/register",
            post(robot::robot_routes::register_robot),
        )
        .route(
            "/ws/robot/control",
            get(robot::client_routes::robot_control_ws),
        )
        .route(
            "/ws/drive/manual",
            get(robot::client_routes::manual_control_ws),
        );

    // robot control routes (called by authenticated user)
    let robot_control_routes = Router::new()
        .route("/nodes", get(robot::client_routes::get_nodes))
        .route("/routes", get(robot::queue_routes::get_routes))
        .route("/routes", post(robot::queue_routes::add_route))
        .route("/routes/{id}", delete(robot::queue_routes::delete_route))
        .route(
            "/routes/optimize",
            post(robot::queue_routes::optimize_routes),
        )
        .route("/routes/select", post(robot::client_routes::select_route))
        .route("/drive/lock", post(robot::client_routes::acquire_lock))
        .route("/drive/lock", delete(robot::client_routes::release_lock))
        .route(
            "/robot/check",
            get(robot::client_routes::check_robot_connection),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(admin_routes)
        .merge(robot_api_routes)
        .merge(robot_control_routes)
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn root() -> &'static str {
    "TeleTable Backend API - v0.1.0"
}
