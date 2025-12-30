use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

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
    #[serde(rename = "SET_MODE")]
    SetMode { mode: String },
    #[serde(rename = "DRIVE_COMMAND")]
    DriveCommand { linear_velocity: f64, angular_velocity: f64 },
}
