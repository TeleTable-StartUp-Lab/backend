pub mod client_routes;
pub mod models;
mod optimization_helper;
pub mod queue_routes;
pub mod robot_routes;
pub mod state;

use crate::AppState;
use models::RobotCommand;
use std::sync::Arc;

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
