use super::models::{QueuedRoute, RobotCommand, RobotState};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SharedRobotState {
    pub current_state: Arc<RwLock<Option<RobotState>>>,
    pub manual_lock: Arc<RwLock<Option<LockInfo>>>,
    pub command_sender: broadcast::Sender<RobotCommand>,
    pub robot_url: Arc<RwLock<Option<String>>>,
    pub cached_nodes: Arc<RwLock<Option<Vec<String>>>>,
    pub queue: Arc<RwLock<VecDeque<QueuedRoute>>>,
    pub active_route: Arc<RwLock<Option<QueuedRoute>>>,
}

#[derive(Debug, Clone)]
pub struct LockInfo {
    pub holder_id: Uuid,
    pub holder_name: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl SharedRobotState {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self {
            current_state: Arc::new(RwLock::new(None)),
            manual_lock: Arc::new(RwLock::new(None)),
            command_sender: tx,
            cached_nodes: Arc::new(RwLock::new(None)),
            robot_url: Arc::new(RwLock::new(None)),
            queue: Arc::new(RwLock::new(VecDeque::new())),
            active_route: Arc::new(RwLock::new(None)),
        }
    }
}

impl Default for SharedRobotState {
    fn default() -> Self {
        Self::new()
    }
}
