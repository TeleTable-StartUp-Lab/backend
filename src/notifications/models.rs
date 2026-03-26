use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RobotNotification {
    pub id: Uuid,
    pub priority: String,
    pub message: String,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct NotificationHistoryQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
