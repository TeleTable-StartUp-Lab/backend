use backend::{create_router, AppState, Config, SharedRobotState};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{sync::Arc, time::Duration};

#[allow(dead_code)]
pub struct TestApp {
    pub router: axum::Router,
    pub db: PgPool,
    pub state: Arc<AppState>,
}

pub async fn spawn_app(pool: PgPool) -> Result<TestApp, String> {
    // Mock Redis or use a real one if available.
    // For tests, we might skip redis if it's only for specific features not tested here,
    // but AppState requires it.
    // We'll assume a local redis is available or mock it via a wrapper if we could.
    // Since we can't easily mock ConnectionManager, we'll try to connect to localhost:6379
    // or use a separate test redis.

    // For simplicity in this environment, we might fail if redis isn't there.
    // Let's assume we can connect to standard redis or use a mocked object if we refactor AppState.
    // Refactoring AppState to use a Trait for Redis would be better, but for now:
    let redis_client = redis::Client::open("redis://127.0.0.1/")
        .map_err(|e| format!("Invalid Redis URL: {e}"))?;
    let redis = redis_client
        .get_connection_manager()
        .await
        .map_err(|e| format!("Failed to connect to Redis: {e}"))?;

    let config = Config {
        database_url: "postgres://...".to_string(), // Overridden by logic elsewhere
        redis_url: "redis://127.0.0.1/".to_string(),
        jwt_secret: "test_secret".to_string(),
        jwt_expiry_hours: 24,
        server_address: "127.0.0.1:0".to_string(),
        robot_api_key: "test_robot_api_key".to_string(),
    };

    let robot_state = SharedRobotState::new();

    // Create HTTP client for tests
    let http_client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(10)
        .build()
        .expect("Failed to create HTTP client");

    let state = Arc::new(AppState {
        db: pool.clone(),
        redis,
        config,
        robot_state: robot_state.clone(),
        http_client,
    });

    let router = create_router(state.clone());

    Ok(TestApp {
        router,
        db: pool,
        state,
    })
}

pub async fn setup_test_app() -> Result<TestApp, String> {
    let database_url = std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .map_err(|_| "TEST_DATABASE_URL or DATABASE_URL must be set for integration tests".to_string())?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await
        .map_err(|e| format!("Failed to connect to database: {e}"))?;

    sqlx::migrate!()
        .run(&pool)
        .await
        .map_err(|e| format!("Failed to run migrations: {e}"))?;

    spawn_app(pool).await
}
