use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

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

#[derive(Debug, Deserialize)]
pub struct CreateDiaryRequest {
    pub id: Option<Uuid>,
    pub working_minutes: i32,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct DiaryQuery {
    pub id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteDiaryRequest {
    pub id: Uuid,
}
