use redis::aio::ConnectionManager;
use tokio::time::sleep;
use tokio::time::Duration;

use sqlx::{postgres::PgPoolOptions, PgPool};

pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}

pub async fn create_redis_client(redis_url: &str) -> ConnectionManager {
    let max_retries = 10;
    let base_delay_ms = 500;

    for attempt in 0..max_retries {
        match redis::Client::open(redis_url) {
            Ok(client) => {
                tracing::info!("Connected to Redis after {} attempt(s)", attempt + 1);
                match ConnectionManager::new(client).await {
                    Ok(connection) => return connection,
                    Err(e) => tracing::warn!("Failed to create Redis connection manager: {}", e),
                }
            }
            Err(e) => tracing::warn!("Failed to create Redis client: {}", e),
        }

        if attempt < max_retries - 1 {
            let delay_ms = base_delay_ms * 2u64.pow(attempt) + rand::random::<u64>() % 500;
            tracing::info!(
                "Retrying Redis in {}ms (attempt {}/{})",
                delay_ms,
                attempt + 1,
                max_retries
            );
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    panic!("Failed to connect to Redis after {} attempts", max_retries);
}
