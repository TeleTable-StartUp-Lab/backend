pub mod client_routes;
pub mod models;
pub mod queue_routes;
pub mod robot_routes;
pub mod state;

use crate::AppState;
use models::RobotCommand;
use std::sync::Arc;

pub async fn process_queue(state: &Arc<AppState>) {
    // 1. Check Manual Lock
    if state.robot_state.manual_lock.read().await.is_some() {
        return;
    }

    // 2. Check if Robot is IDLE
    // logic: We need the drive_mode to be IDLE to start a new command
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

    // 3. Check Active Route (should be None if we want to start one)
    let mut active_route_guard = state.robot_state.active_route.write().await;
    if active_route_guard.is_some() {
        return;
    }

    // 4. Pop from Queue
    let mut queue = state.robot_state.queue.write().await;
    if let Some(next_route) = queue.pop_front() {
        // 5. Send Command
        let cmd = RobotCommand::Navigate {
            start: next_route.start.clone(),
            destination: next_route.destination.clone(),
        };

        match state.robot_state.command_sender.send(cmd) {
            Ok(_) => {
                // 6. Set Active
                *active_route_guard = Some(next_route);
                tracing::info!("Dispatched route from queue");
            }
            Err(e) => {
                tracing::error!("Failed to dispatch route command: {}", e);
                // Push back to front?
                queue.push_front(next_route);
            }
        }
    }
}
