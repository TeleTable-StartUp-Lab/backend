use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

const USER_CACHE_TTL: u64 = 300; // 5 minutes
const JWT_CACHE_TTL: u64 = 3600; // 1 hour
const DIARY_CACHE_TTL: u64 = 60; // 1 minute
const NODES_CACHE_TTL: u64 = 600; // 10 minutes

pub struct CacheService;

impl CacheService {
    /// Cache user data by user ID
    pub async fn cache_user<T: Serialize>(
        redis: &mut ConnectionManager,
        user_id: &str,
        user_data: &T,
    ) -> Result<(), redis::RedisError> {
        let key = format!("user:{}", user_id);
        let value = serde_json::to_string(user_data).unwrap_or_default();
        redis.set_ex(key, value, USER_CACHE_TTL).await
    }

    /// Get cached user data
    pub async fn get_user<T: for<'de> Deserialize<'de>>(
        redis: &mut ConnectionManager,
        user_id: &str,
    ) -> Result<Option<T>, redis::RedisError> {
        let key = format!("user:{}", user_id);
        let value: Option<String> = redis.get(key).await?;
        Ok(value.and_then(|v| serde_json::from_str(&v).ok()))
    }

    /// Invalidate user cache
    pub async fn invalidate_user(
        redis: &mut ConnectionManager,
        user_id: &str,
    ) -> Result<(), redis::RedisError> {
        let key = format!("user:{}", user_id);
        redis.del(key).await
    }

    /// Cache JWT validation result
    pub async fn cache_jwt_validation(
        redis: &mut ConnectionManager,
        token_hash: &str,
        claims: &str,
    ) -> Result<(), redis::RedisError> {
        let key = format!("jwt:{}", token_hash);
        redis.set_ex(key, claims, JWT_CACHE_TTL).await
    }

    /// Get cached JWT validation
    pub async fn get_jwt_validation(
        redis: &mut ConnectionManager,
        token_hash: &str,
    ) -> Result<Option<String>, redis::RedisError> {
        let key = format!("jwt:{}", token_hash);
        redis.get(key).await
    }

    /// Cache diary entry
    pub async fn cache_diary<T: Serialize>(
        redis: &mut ConnectionManager,
        diary_id: &str,
        diary_data: &T,
    ) -> Result<(), redis::RedisError> {
        let key = format!("diary:{}", diary_id);
        let value = serde_json::to_string(diary_data).unwrap_or_default();
        redis.set_ex(key, value, DIARY_CACHE_TTL).await
    }

    /// Get cached diary entry
    pub async fn get_diary<T: for<'de> Deserialize<'de>>(
        redis: &mut ConnectionManager,
        diary_id: &str,
    ) -> Result<Option<T>, redis::RedisError> {
        let key = format!("diary:{}", diary_id);
        let value: Option<String> = redis.get(key).await?;
        Ok(value.and_then(|v| serde_json::from_str(&v).ok()))
    }

    /// Invalidate diary cache
    pub async fn invalidate_diary(
        redis: &mut ConnectionManager,
        diary_id: &str,
    ) -> Result<(), redis::RedisError> {
        let key = format!("diary:{}", diary_id);
        redis.del(key).await
    }

    /// Invalidate all diaries for a user
    pub async fn invalidate_user_diaries(
        redis: &mut ConnectionManager,
        user_id: &str,
    ) -> Result<(), redis::RedisError> {
        let pattern = format!("diary:user:{}:*", user_id);
        let keys: Vec<String> = redis.keys(pattern).await?;
        if !keys.is_empty() {
            redis.del::<_, ()>(keys).await?;
        }
        Ok(())
    }

    /// Cache robot nodes
    pub async fn cache_nodes(
        redis: &mut ConnectionManager,
        nodes: &[String],
    ) -> Result<(), redis::RedisError> {
        let key = "robot:nodes";
        let value = serde_json::to_string(nodes).unwrap_or_default();
        redis.set_ex(key, value, NODES_CACHE_TTL).await
    }

    /// Get cached nodes
    pub async fn get_nodes(
        redis: &mut ConnectionManager,
    ) -> Result<Option<Vec<String>>, redis::RedisError> {
        let key = "robot:nodes";
        let value: Option<String> = redis.get(key).await?;
        Ok(value.and_then(|v| serde_json::from_str(&v).ok()))
    }
}
