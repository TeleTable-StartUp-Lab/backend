use sqlx::{postgres::PgPoolOptions, PgPool};
use redis::aio::ConnectionManager;

pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
}

pub async fn create_redis_client(redis_url: &str) -> Result<ConnectionManager, redis::RedisError> {
    let client = redis::Client::open(redis_url)?;
    ConnectionManager::new(client).await
}
