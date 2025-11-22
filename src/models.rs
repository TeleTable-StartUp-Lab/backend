use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub password_hash: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub role: String,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        UserResponse {
            id: user.id,
            name: user.name,
            email: user.email,
            role: user.role,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DiaryEntry {
    pub id: Uuid,
    pub owner: Uuid,
    pub working_minutes: i32,
    pub text: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiaryResponse {
    pub id: Uuid,
    pub owner: Uuid,
    pub working_minutes: i32,
    pub text: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DiaryEntry> for DiaryResponse {
    fn from(entry: DiaryEntry) -> Self {
        DiaryResponse {
            id: entry.id,
            owner: entry.owner,
            working_minutes: entry.working_minutes,
            text: entry.text,
            created_at: entry.created_at,
            updated_at: entry.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // User ID
    pub name: String,
    pub role: String,
    pub exp: usize, // Expiration time
}

