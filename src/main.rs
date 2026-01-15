use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

mod auth;
mod config;
mod database;
mod diary;
mod robot;

use auth::auth::{admin_middleware, auth_middleware};
use config::Config;
use database::{create_pool, create_redis_client};
use robot::state::SharedRobotState;

pub struct AppState {
    pub db: PgPool,
    pub redis: ConnectionManager,
    pub config: Config,
    pub robot_state: SharedRobotState,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();
    let config = Config::from_env().expect("Failed to load configuration");

    let db = create_pool(&config.database_url)
        .await
        .expect("Failed to create database pool");

    let redis = create_redis_client(&config.redis_url)
        .await
        .expect("Failed to create redis client");

    info!("Connected to database and redis");
    info!("Running database migrations...");
    // Load migrations from the filesystem so we don't need to rebuild the binary for every new migration
    let migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
        .await
        .expect("Failed to load migrations");

    match migrator.run(&db).await {
        Ok(_) => info!("Migrations completed successfully"),
        Err(e) => {
            tracing::error!("Migration error: {e}");
            panic!("Failed to run migrations: {e}");
        }
    }

    let robot_state = SharedRobotState::new();
    let state = Arc::new(AppState {
        db,
        redis,
        config,
        robot_state: robot_state.clone(),
    });

    // Start UDP discovery service
    tokio::spawn(robot::discovery::run_discovery_service(robot_state));

    // public routes (no authentication required)
    let public_routes = Router::new()
        .route("/", get(root))
        .route("/register", post(auth::login::register))
        .route("/login", post(auth::login::login));

    // protected routes (authentication required)
    let protected_routes = Router::new()
        .route("/me", get(auth::login::get_me))
        .route("/diary", post(diary::diary::create_or_update_diary))
        .route("/diary", get(diary::diary::get_diary))
        .route("/diary", delete(diary::diary::delete_diary))
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

    // Combine all routes
    let app = Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(admin_routes)
        .merge(robot_api_routes)
        .merge(robot_control_routes)
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let server_address = state.config.server_address.clone();
    info!("Starting server on {server_address}");

    let listener = tokio::net::TcpListener::bind(&server_address)
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root() -> &'static str {
    "TeleTable Backend API - v0.1.0"
}
