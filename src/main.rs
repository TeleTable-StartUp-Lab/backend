use backend::{
    create_pool, create_redis_client, create_router, AppState, Config, SharedRobotState,
};
use std::sync::Arc;
use tracing::info;

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
        config: config.clone(),
        robot_state: robot_state.clone(),
    });

    let app = create_router(state);

    let server_address = config.server_address.clone();
    info!("Starting server on {server_address}");

    let listener = tokio::net::TcpListener::bind(&server_address)
        .await
        .unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .unwrap();
}
