use super::models::{QueuedRoute, RobotCommand, RobotState, RobotStatusUpdate};
use crate::notifications::models::RobotNotification;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

/// How many seconds without a state update before the robot is considered disconnected
pub const ROBOT_STALE_TIMEOUT_SECS: i64 = 30;
/// How often the background cleanup task runs (in seconds)
pub const CLEANUP_INTERVAL_SECS: u64 = 5;
#[derive(Debug, Clone)]
pub struct SharedRobotState {
    pub current_state: Arc<RwLock<Option<RobotState>>>,
    pub last_state_update: Arc<RwLock<Option<DateTime<Utc>>>>,
    pub manual_lock: Arc<RwLock<Option<LockInfo>>>,
    pub command_sender: broadcast::Sender<RobotCommand>,
    pub status_sender: broadcast::Sender<RobotStatusUpdate>,
    pub notification_sender: broadcast::Sender<RobotNotification>,
    pub robot_url: Arc<RwLock<Option<String>>>,
    pub control_channel_connections: Arc<AtomicUsize>,
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
        let (command_tx, _) = broadcast::channel(100);
        let (status_tx, _) = broadcast::channel(200);
        let (notification_tx, _) = broadcast::channel(200);
        Self {
            current_state: Arc::new(RwLock::new(None)),
            last_state_update: Arc::new(RwLock::new(None)),
            manual_lock: Arc::new(RwLock::new(None)),
            command_sender: command_tx,
            status_sender: status_tx,
            notification_sender: notification_tx,
            robot_url: Arc::new(RwLock::new(None)),
            control_channel_connections: Arc::new(AtomicUsize::new(0)),
            queue: Arc::new(RwLock::new(VecDeque::new())),
            active_route: Arc::new(RwLock::new(None)),
        }
    }

    /// Returns true if the robot has sent a state update within the staleness threshold
    pub async fn is_robot_connected(&self) -> bool {
        let last_update = self.last_state_update.read().await;
        match *last_update {
            Some(t) => (Utc::now() - t).num_seconds() < ROBOT_STALE_TIMEOUT_SECS,
            None => false,
        }
    }

    pub fn register_control_channel_connection(&self) {
        self.control_channel_connections
            .fetch_add(1, Ordering::SeqCst);
    }

    pub fn unregister_control_channel_connection(&self) {
        let _ = self.control_channel_connections.fetch_update(
            Ordering::SeqCst,
            Ordering::SeqCst,
            |current| current.checked_sub(1),
        );
    }

    pub fn is_control_channel_connected(&self) -> bool {
        self.control_channel_connections.load(Ordering::SeqCst) > 0
    }

    /// Clear an expired manual lock. Returns true if a lock was cleared.
    pub async fn clear_expired_lock(&self) -> bool {
        let mut lock = self.manual_lock.write().await;
        if let Some(l) = &*lock {
            if l.expires_at <= Utc::now() {
                tracing::info!("Clearing expired lock held by {}", l.holder_name);
                *lock = None;
                return true;
            }
        }
        false
    }
}

impl Default for SharedRobotState {
    fn default() -> Self {
        Self::new()
    }
}
