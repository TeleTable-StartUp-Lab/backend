pub mod client_routes;
pub mod models;
mod optimization_helper;
pub mod queue_routes;
pub mod robot_routes;
pub mod state;

use crate::AppState;
use models::{LastRoute, RobotCommand, RobotStatusUpdate};
use std::sync::Arc;

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

    let nodes = get_or_refresh_nodes(state).await;

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

async fn get_or_refresh_nodes(state: &Arc<AppState>) -> Vec<String> {
    if let Some(nodes) = &*state.robot_state.cached_nodes.read().await {
        return nodes.clone();
    }

    let mut redis = state.redis.clone();
    if let Ok(Some(nodes)) = crate::cache::CacheService::get_nodes(&mut redis).await {
        let mut cache = state.robot_state.cached_nodes.write().await;
        *cache = Some(nodes.clone());
        return nodes;
    }

    let robot_url = state.robot_state.robot_url.read().await.clone();
    if let Some(url) = robot_url {
        if let Ok(resp) = state.http_client.get(format!("{url}/nodes")).send().await {
            if resp.status().is_success() {
                if let Ok(nodes_resp) = resp.json::<models::NodesResponse>().await {
                    let nodes = nodes_resp.nodes;
                    let _ = crate::cache::CacheService::cache_nodes(&mut redis, &nodes).await;
                    let mut cache = state.robot_state.cached_nodes.write().await;
                    *cache = Some(nodes.clone());
                    return nodes;
                }
            }
        }
    }

    Vec::new()
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
