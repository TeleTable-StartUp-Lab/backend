use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct QueuedRoute {
    pub id: Uuid,
    pub start: String,
    pub destination: String,
    pub added_at: DateTime<Utc>,
    pub added_by: String, // User name or ID
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotState {
    pub system_health: String,
    pub battery_level: u8,
    pub drive_mode: String,
    pub cargo_status: String,
    pub current_position: String,
    pub last_node: Option<String>,
    pub target_node: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotEvent {
    pub event: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "command")]
pub enum RobotCommand {
    #[serde(rename = "NAVIGATE")]
    Navigate { start: String, destination: String },
    #[serde(rename = "CANCEL")]
    Cancel,
    #[serde(rename = "DRIVE_COMMAND")]
    DriveCommand {
        linear_velocity: f64,
        angular_velocity: f64,
    },
    #[serde(rename = "LED")]
    Led {
        enabled: bool,
        r: u8,
        g: u8,
        b: u8,
        brightness: u8,
    },
    #[serde(rename = "AUDIO_BEEP")]
    AudioBeep { hz: u32, ms: u32 },
    #[serde(rename = "AUDIO_VOLUME")]
    AudioVolume { value: f32 },
}

#[derive(Debug, Serialize)]
pub struct LastRoute {
    pub start_node: String,
    pub end_node: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub system_health: String,
    pub battery_level: u8,
    pub drive_mode: String,
    pub cargo_status: String,
    pub last_route: Option<LastRoute>,
    pub position: String,
    pub manual_lock_holder_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodesResponse {
    pub nodes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RouteSelectionRequest {
    pub start: String,
    pub destination: String,
}
