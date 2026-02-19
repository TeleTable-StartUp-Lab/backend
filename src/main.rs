use backend::{
    create_pool, create_redis_client, create_router, logging, AppState, Config, SharedRobotState,
};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Initialise logging first – guard must live for the entire process lifetime.
    let _logging_guard = logging::init();

    // Capture panics so they appear in the structured log (stdout + file) with
    // a full backtrace before the process exits.
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".to_string());

        let message = panic_info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .unwrap_or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(|s| s.as_str())
                    .unwrap_or("(non-string panic payload)")
            });

        // RUST_BACKTRACE is not required; we force-capture here.
        let backtrace = std::backtrace::Backtrace::force_capture();

        tracing::error!(
            location = %location,
            message  = %message,
            backtrace = %backtrace,
            "PANIC - uncaught exception"
        );
    }));

    dotenv::dotenv().ok();
    let config = Config::from_env().expect("Failed to load configuration");

    // Log server configuration (never log secret values).
    tracing::info!(
        server_address  = %config.server_address,
        jwt_expiry_hours = config.jwt_expiry_hours,
        "Server configuration loaded"
    );

    let db = create_pool(&config.database_url)
        .await
        .expect("Failed to create database pool");
    tracing::info!("Connected to database");

    let redis = create_redis_client(&config.redis_url)
        .await
        .expect("Failed to create Redis client");
    tracing::info!("Connected to Redis");

    tracing::info!("Running database migrations...");
    let migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
        .await
        .expect("Failed to load migrations");

    match migrator.run(&db).await {
        Ok(_) => tracing::info!("Migrations completed successfully"),
        Err(e) => {
            tracing::error!(error = %e, "Database migration failed");
            panic!("Failed to run migrations: {e}");
        }
    }

    let robot_state = SharedRobotState::new();

    // Create reusable HTTP client with optimised settings.
    let http_client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .pool_max_idle_per_host(10)
        .build()
        .expect("Failed to create HTTP client");

    let state = Arc::new(AppState {
        db,
        redis,
        config: config.clone(),
        robot_state: robot_state.clone(),
        http_client,
    });

    let app = create_router(state);

    let server_address = config.server_address.clone();
    tracing::info!(address = %server_address, "Starting server");

    let listener = tokio::net::TcpListener::bind(&server_address)
        .await
        .expect("Failed to bind server address");

    tracing::info!(address = %server_address, "Server started successfully");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("Server error");

    tracing::info!("Server stopped");
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C signal handler");
    tracing::info!("Shutdown signal received, stopping server...");
}
