use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedMode {
    Static,
    Breathing,
    Loop,
    Rainbow,
}

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
    #[serde(default)]
    pub gyroscope: Option<RobotGyroscopeReading>,
    #[serde(default)]
    pub last_read_uuid: Option<String>,
    #[serde(default)]
    pub lux: Option<f32>,
    #[serde(default)]
    pub infrared: Option<RobotInfraredReading>,
    #[serde(default)]
    pub voltage_v: Option<f32>,
    #[serde(default)]
    pub current_a: Option<f32>,
    #[serde(default)]
    pub power_w: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RobotGyroscopeReading {
    #[serde(default)]
    pub x_dps: Option<f32>,
    #[serde(default)]
    pub y_dps: Option<f32>,
    #[serde(default)]
    pub z_dps: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RobotInfraredReading {
    #[serde(default)]
    pub front: Option<bool>,
    #[serde(default)]
    pub left: Option<bool>,
    #[serde(default)]
    pub right: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RobotEvent {
    pub priority: RobotEventPriority,
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "UPPERCASE")]
pub enum RobotEventPriority {
    Info,
    Warn,
    Error,
}

impl RobotEventPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            RobotEventPriority::Info => "INFO",
            RobotEventPriority::Warn => "WARN",
            RobotEventPriority::Error => "ERROR",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct RobotNode {
    pub id: String,
    pub label: String,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<LedMode>,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LastRoute {
    pub start_node: String,
    pub end_node: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotStatusUpdate {
    pub system_health: String,
    pub battery_level: u8,
    pub drive_mode: String,
    pub cargo_status: String,
    pub position: String,
    pub last_route: Option<LastRoute>,
    pub manual_lock_holder_name: Option<String>,
    pub robot_connected: bool,
    pub nodes: Vec<RobotNode>,
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
    pub robot_connected: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodesResponse {
    pub nodes: Vec<RobotNode>,
}

#[derive(Debug, Deserialize)]
pub struct RouteSelectionRequest {
    pub start: String,
    pub destination: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotDebugSnapshot {
    pub telemetry: RobotDebugTelemetry,
    pub lock: RobotDebugLock,
    pub routing: RobotDebugRouting,
    pub connection: RobotDebugConnection,
    pub sensors: RobotDebugSensors,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotDebugTelemetry {
    pub system_health: String,
    pub battery_level: u8,
    pub drive_mode: String,
    pub cargo_status: String,
    pub position: String,
    pub last_route: Option<LastRoute>,
    pub robot_connected: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotDebugLock {
    pub holder_name: Option<String>,
    pub active: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotDebugRouting {
    pub active_route: Option<QueuedRoute>,
    pub queue: Vec<QueuedRoute>,
    pub queue_length: usize,
    pub nodes: Vec<RobotNode>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotDebugConnection {
    pub robot_url: Option<String>,
    pub last_state_update: Option<DateTime<Utc>>,
    pub robot_status_reachable: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotDebugSensors {
    pub light: RobotDebugLightSensor,
    pub infrared: RobotDebugInfraredSensor,
    pub power: RobotDebugPowerSensor,
    pub gyroscope: RobotDebugGyroscopeSensor,
    pub rfid: RobotDebugRfidSensor,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotDebugLightSensor {
    pub lux: Option<f32>,
    pub valid: bool,
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotDebugInfraredSensor {
    pub front: Option<bool>,
    pub left: Option<bool>,
    pub right: Option<bool>,
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotDebugPowerSensor {
    pub voltage_v: Option<f32>,
    pub current_a: Option<f32>,
    pub power_w: Option<f32>,
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotDebugGyroscopeSensor {
    pub x_dps: Option<f32>,
    pub y_dps: Option<f32>,
    pub z_dps: Option<f32>,
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotDebugRfidSensor {
    pub last_read_uuid: Option<String>,
    pub source: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotStatusHttpResponse {
    pub sensors: Option<RobotStatusHttpSensors>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotStatusHttpSensors {
    pub ir: Option<RobotStatusHttpInfrared>,
    pub light: Option<RobotStatusHttpLight>,
    pub power: Option<RobotStatusHttpPower>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotStatusHttpInfrared {
    pub left: bool,
    pub middle: bool,
    pub right: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotStatusHttpLight {
    pub lux_valid: bool,
    pub lux: f32,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobotStatusHttpPower {
    pub valid: bool,
    pub battery_voltage: f32,
    pub current_a: f32,
    pub power_w: f32,
}
