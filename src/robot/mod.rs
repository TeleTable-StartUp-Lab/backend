pub mod client_routes;
pub mod models;
mod optimization_helper;
pub mod queue_routes;
pub mod robot_routes;
pub mod state;

use crate::AppState;
use models::{
    LastRoute, RobotCommand, RobotDebugConnection, RobotDebugGyroscopeSensor,
    RobotDebugInfraredSensor, RobotDebugLightSensor, RobotDebugLock, RobotDebugPowerSensor,
    RobotDebugRfidSensor, RobotDebugRouting, RobotDebugSensors, RobotDebugSnapshot,
    RobotDebugTelemetry, RobotStatusHttpResponse, RobotStatusUpdate,
};
use std::sync::Arc;
use std::time::Duration;

const SENSOR_SOURCE_ROBOT_STATUS_HTTP: &str = "robot_status_http";
const SENSOR_SOURCE_TABLE_STATE: &str = "table_state";
const SENSOR_SOURCE_UNAVAILABLE: &str = "unavailable";
const ROBOT_STATUS_TIMEOUT_SECS: u64 = 2;

pub async fn build_status_update(state: &Arc<AppState>) -> RobotStatusUpdate {
    let robot_state = state.robot_state.current_state.read().await;
    let lock_state = state.robot_state.manual_lock.read().await;
    let robot_connected = state.robot_state.is_robot_connected().await;

    let (system_health, battery_level, drive_mode, cargo_status, position, last_route) =
        if let Some(rs) = &*robot_state {
            (
                rs.system_health.clone(),
                rs.battery_level,
                rs.drive_mode.clone(),
                rs.cargo_status.clone(),
                rs.current_position.clone(),
                if let (Some(start), Some(end)) = (&rs.last_node, &rs.target_node) {
                    Some(LastRoute {
                        start_node: start.clone(),
                        end_node: end.clone(),
                    })
                } else {
                    None
                },
            )
        } else {
            (
                "UNKNOWN".to_string(),
                0,
                "UNKNOWN".to_string(),
                "UNKNOWN".to_string(),
                "UNKNOWN".to_string(),
                None,
            )
        };

    let manual_lock_holder_name = lock_state
        .as_ref()
        .filter(|l| l.expires_at > chrono::Utc::now())
        .map(|l| l.holder_name.clone());

    let nodes = state.static_nodes.clone();

    RobotStatusUpdate {
        system_health,
        battery_level,
        drive_mode,
        cargo_status,
        position,
        last_route,
        manual_lock_holder_name,
        robot_connected,
        nodes,
    }
}

pub async fn broadcast_status_update(state: &Arc<AppState>) {
    let status_update = build_status_update(state).await;
    let _ = state.robot_state.status_sender.send(status_update);
}

async fn fetch_robot_status(
    state: &Arc<AppState>,
    robot_url: Option<&str>,
) -> Option<RobotStatusHttpResponse> {
    let url = robot_url?;
    let endpoint = format!("{url}/status");
    let response = match state
        .http_client
        .get(&endpoint)
        .timeout(Duration::from_secs(ROBOT_STATUS_TIMEOUT_SECS))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(
                endpoint = %endpoint,
                error = %error,
                "External API failure - could not reach robot /status"
            );
            return None;
        }
    };

    if !response.status().is_success() {
        tracing::warn!(
            endpoint = %endpoint,
            status_code = response.status().as_u16(),
            "External API failure - robot /status returned non-success status"
        );
        return None;
    }

    match response.json::<RobotStatusHttpResponse>().await {
        Ok(status) => Some(status),
        Err(error) => {
            tracing::warn!(
                endpoint = %endpoint,
                error = %error,
                "External API failure - robot /status returned invalid JSON"
            );
            None
        }
    }
}

pub async fn build_debug_snapshot(state: &Arc<AppState>) -> RobotDebugSnapshot {
    let current_state = state.robot_state.current_state.read().await.clone();
    let robot_connected = state.robot_state.is_robot_connected().await;
    let last_state_update = *state.robot_state.last_state_update.read().await;
    let robot_url = state.robot_state.robot_url.read().await.clone();
    let active_route = state.robot_state.active_route.read().await.clone();
    let queue = state
        .robot_state
        .queue
        .read()
        .await
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let lock = state.robot_state.manual_lock.read().await.clone();
    let nodes = state.static_nodes.clone();
    let robot_status = fetch_robot_status(state, robot_url.as_deref()).await;
    let robot_status_reachable = robot_status.is_some();
    let now = chrono::Utc::now();

    let (system_health, battery_level, drive_mode, cargo_status, position, last_route) =
        if let Some(rs) = &current_state {
            (
                rs.system_health.clone(),
                rs.battery_level,
                rs.drive_mode.clone(),
                rs.cargo_status.clone(),
                rs.current_position.clone(),
                if let (Some(start), Some(end)) = (&rs.last_node, &rs.target_node) {
                    Some(LastRoute {
                        start_node: start.clone(),
                        end_node: end.clone(),
                    })
                } else {
                    None
                },
            )
        } else {
            (
                "UNKNOWN".to_string(),
                0,
                "UNKNOWN".to_string(),
                "UNKNOWN".to_string(),
                "UNKNOWN".to_string(),
                None,
            )
        };

    let (lock_active, holder_name, expires_at) = match lock {
        Some(lock_info) if lock_info.expires_at > now => (
            true,
            Some(lock_info.holder_name),
            Some(lock_info.expires_at),
        ),
        Some(lock_info) => (
            false,
            Some(lock_info.holder_name),
            Some(lock_info.expires_at),
        ),
        None => (false, None, None),
    };

    let light_sensor = match (
        current_state.as_ref().and_then(|state| state.lux),
        robot_status
            .as_ref()
            .and_then(|status| status.sensors.as_ref())
            .and_then(|sensors| sensors.light.as_ref()),
    ) {
        (Some(lux), _) => RobotDebugLightSensor {
            lux: Some(lux),
            valid: true,
            source: SENSOR_SOURCE_TABLE_STATE.to_string(),
        },
        (None, Some(light)) => RobotDebugLightSensor {
            lux: light.lux_valid.then_some(light.lux),
            valid: light.lux_valid,
            source: SENSOR_SOURCE_ROBOT_STATUS_HTTP.to_string(),
        },
        (None, None) => RobotDebugLightSensor {
            lux: None,
            valid: false,
            source: SENSOR_SOURCE_UNAVAILABLE.to_string(),
        },
    };

    let infrared_sensor = if let Some(ir) = current_state
        .as_ref()
        .and_then(|state| state.infrared.as_ref())
    {
        RobotDebugInfraredSensor {
            front: ir.front,
            left: ir.left,
            right: ir.right,
            source: SENSOR_SOURCE_TABLE_STATE.to_string(),
        }
    } else if let Some(ir) = robot_status
        .as_ref()
        .and_then(|status| status.sensors.as_ref())
        .and_then(|sensors| sensors.ir.as_ref())
    {
        RobotDebugInfraredSensor {
            front: Some(ir.middle),
            left: Some(ir.left),
            right: Some(ir.right),
            source: SENSOR_SOURCE_ROBOT_STATUS_HTTP.to_string(),
        }
    } else {
        RobotDebugInfraredSensor {
            front: None,
            left: None,
            right: None,
            source: SENSOR_SOURCE_UNAVAILABLE.to_string(),
        }
    };

    let power_sensor = if let Some(state) = current_state.as_ref() {
        let has_power =
            state.voltage_v.is_some() || state.current_a.is_some() || state.power_w.is_some();
        if has_power {
            RobotDebugPowerSensor {
                voltage_v: state.voltage_v,
                current_a: state.current_a,
                power_w: state.power_w,
                source: SENSOR_SOURCE_TABLE_STATE.to_string(),
            }
        } else if let Some(power) = robot_status
            .as_ref()
            .and_then(|status| status.sensors.as_ref())
            .and_then(|sensors| sensors.power.as_ref())
        {
            RobotDebugPowerSensor {
                voltage_v: power.valid.then_some(power.battery_voltage),
                current_a: power.valid.then_some(power.current_a),
                power_w: power.valid.then_some(power.power_w),
                source: SENSOR_SOURCE_ROBOT_STATUS_HTTP.to_string(),
            }
        } else {
            RobotDebugPowerSensor {
                voltage_v: None,
                current_a: None,
                power_w: None,
                source: SENSOR_SOURCE_UNAVAILABLE.to_string(),
            }
        }
    } else if let Some(power) = robot_status
        .as_ref()
        .and_then(|status| status.sensors.as_ref())
        .and_then(|sensors| sensors.power.as_ref())
    {
        RobotDebugPowerSensor {
            voltage_v: power.valid.then_some(power.battery_voltage),
            current_a: power.valid.then_some(power.current_a),
            power_w: power.valid.then_some(power.power_w),
            source: SENSOR_SOURCE_ROBOT_STATUS_HTTP.to_string(),
        }
    } else {
        RobotDebugPowerSensor {
            voltage_v: None,
            current_a: None,
            power_w: None,
            source: SENSOR_SOURCE_UNAVAILABLE.to_string(),
        }
    };

    let gyroscope = current_state
        .as_ref()
        .and_then(|state| state.gyroscope.as_ref());
    let gyroscope_sensor = RobotDebugGyroscopeSensor {
        x_dps: gyroscope.and_then(|reading| reading.x_dps),
        y_dps: gyroscope.and_then(|reading| reading.y_dps),
        z_dps: gyroscope.and_then(|reading| reading.z_dps),
        source: if gyroscope.is_some() {
            SENSOR_SOURCE_TABLE_STATE
        } else {
            SENSOR_SOURCE_UNAVAILABLE
        }
        .to_string(),
    };

    let has_rfid = current_state
        .as_ref()
        .and_then(|state| state.last_read_uuid.as_ref())
        .is_some();
    let rfid_sensor = RobotDebugRfidSensor {
        last_read_uuid: current_state
            .as_ref()
            .and_then(|state| state.last_read_uuid.clone()),
        source: if has_rfid {
            SENSOR_SOURCE_TABLE_STATE
        } else {
            SENSOR_SOURCE_UNAVAILABLE
        }
        .to_string(),
    };

    RobotDebugSnapshot {
        telemetry: RobotDebugTelemetry {
            system_health,
            battery_level,
            drive_mode,
            cargo_status,
            position,
            last_route,
            robot_connected,
        },
        lock: RobotDebugLock {
            holder_name,
            active: lock_active,
            expires_at,
        },
        routing: RobotDebugRouting {
            active_route,
            queue_length: queue.len(),
            queue,
            nodes,
        },
        connection: RobotDebugConnection {
            robot_url,
            last_state_update,
            robot_status_reachable,
        },
        sensors: RobotDebugSensors {
            light: light_sensor,
            infrared: infrared_sensor,
            power: power_sensor,
            gyroscope: gyroscope_sensor,
            rfid: rfid_sensor,
        },
    }
}

pub async fn process_queue(state: &Arc<AppState>) {
    // 1. Check Manual Lock (only if not expired)
    {
        let lock = state.robot_state.manual_lock.read().await;
        if let Some(l) = &*lock {
            if l.expires_at > chrono::Utc::now() {
                return; // Active lock held, don't process queue
            }
        }
    }

    // 2. Don't process queue if robot is disconnected/stale
    if !state.robot_state.is_robot_connected().await {
        return;
    }

    // 3. Check if Robot is IDLE
    let is_idle = {
        let rs = state.robot_state.current_state.read().await;
        match &*rs {
            Some(s) => s.drive_mode == "IDLE",
            None => false, // Can't drive if unknown
        }
    };

    if !is_idle {
        return;
    }

    // 4. Check Active Route (should be None if we want to start one)
    let mut active_route_guard = state.robot_state.active_route.write().await;
    if active_route_guard.is_some() {
        return;
    }

    // 5. Pop from Queue
    let mut queue = state.robot_state.queue.write().await;
    if let Some(next_route) = queue.pop_front() {
        // 6. Send Command
        let cmd = RobotCommand::Navigate {
            start: next_route.start.clone(),
            destination: next_route.destination.clone(),
        };

        match state.robot_state.command_sender.send(cmd) {
            Ok(_) => {
                // 7. Set Active
                tracing::info!(
                    route_id    = %next_route.id,
                    start       = %next_route.start,
                    destination = %next_route.destination,
                    added_by    = %next_route.added_by,
                    "Dispatched route from queue"
                );
                *active_route_guard = Some(next_route);
            }
            Err(e) => {
                tracing::error!(
                    route_id    = %next_route.id,
                    start       = %next_route.start,
                    destination = %next_route.destination,
                    error       = %e,
                    "Failed to dispatch route command - re-queuing"
                );
                // Push back to front?
                queue.push_front(next_route);
            }
        }
    }
}
