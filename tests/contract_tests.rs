use backend::robot::models::{RobotCommand, RobotState};
use serde_json::json;

#[test]
fn test_robot_state_contract() {
    // 1. Simulate Firmware JSON Output
    let json_data = json!({
        "systemHealth": "OK",
        "batteryLevel": 85,
        "driveMode": "MANUAL",
        "cargoStatus": "EMPTY",
        "currentPosition": "Kitchen",
        "lastNode": null,
        "targetNode": null,
        "lux": 150.0 // Extra field shouldn't panic
    });

    // 2. Deserialize into Backend Struct
    let state: RobotState =
        serde_json::from_value(json_data).expect("Failed to deserialize RobotState");

    // 3. Verify mappings
    assert_eq!(state.system_health, "OK");
    assert_eq!(state.battery_level, 85);
    assert_eq!(state.drive_mode, "MANUAL");
    assert_eq!(state.cargo_status, "EMPTY");
    assert_eq!(state.current_position, "Kitchen");
    assert_eq!(state.last_node, None);
}

#[test]
fn test_robot_command_serialization() {
    // 1. Create Backend Command
    let cmd = RobotCommand::Navigate {
        start: "Home".to_string(),
        destination: "Office".to_string(),
    };

    // 2. Serialize
    let json_val = serde_json::to_value(&cmd).expect("Failed to serialize");

    // 3. Verify Contract
    // #[serde(tag = "command")] implies {"command": "NAVIGATE", "start": "Home", "destination": "Office"}
    assert_eq!(json_val["command"], "NAVIGATE");
    assert_eq!(json_val["start"], "Home");
    assert_eq!(json_val["destination"], "Office");
}
