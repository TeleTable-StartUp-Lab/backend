use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

mod common;

/// Helper: create auth header with the test secret
fn auth_header(role: &str) -> String {
    let token =
        backend::auth::security::create_jwt("user-1", "Test User", role, "test_secret", 1).unwrap();
    format!("Bearer {token}")
}

fn auth_header_for(user_id: &str, name: &str, role: &str) -> String {
    let token = backend::auth::security::create_jwt(user_id, name, role, "test_secret", 1).unwrap();
    format!("Bearer {token}")
}

// ---------------------------------------------------------------------------
// 1. Expired lock is not reported in /status
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_status_hides_expired_lock() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping test_status_hides_expired_lock: {e}");
            return;
        }
    };

    // Set a lock that expired 10 seconds ago
    {
        let mut lock = app.state.robot_state.manual_lock.write().await;
        *lock = Some(backend::robot::state::LockInfo {
            holder_id: uuid::Uuid::new_v4(),
            holder_name: "Old User".to_string(),
            expires_at: chrono::Utc::now() - chrono::Duration::seconds(10),
        });
    }

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/status")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // The expired lock holder should NOT appear
    assert!(
        status["manualLockHolderName"].is_null(),
        "Expired lock holder should not be reported in status, got: {:?}",
        status["manualLockHolderName"]
    );
}

// ---------------------------------------------------------------------------
// 2. Active (non-expired) lock IS reported in /status
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_status_shows_active_lock() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    {
        let mut lock = app.state.robot_state.manual_lock.write().await;
        *lock = Some(backend::robot::state::LockInfo {
            holder_id: uuid::Uuid::new_v4(),
            holder_name: "Active User".to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
        });
    }

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/status")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status["manualLockHolderName"], "Active User");
}

// ---------------------------------------------------------------------------
// 3. /status reports robot_connected = false when no state updates received
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_status_robot_disconnected_when_no_updates() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    // No state updates → last_state_update is None
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/status")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status["robotConnected"], false);
}

// ---------------------------------------------------------------------------
// 4. /status reports robot_connected = true after a fresh state update
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_status_robot_connected_after_update() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    // Simulate a robot state update via HTTP
    let payload = serde_json::json!({
        "systemHealth": "OK",
        "batteryLevel": 90,
        "driveMode": "IDLE",
        "cargoStatus": "EMPTY",
        "currentPosition": "kitchen",
        "lastNode": null,
        "targetNode": null
    });

    let _ = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/table/state")
                .method("POST")
                .header("Content-Type", "application/json")
                .header("X-Api-Key", "test_robot_api_key")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Now check /status
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/status")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status["robotConnected"], true);
}

// ---------------------------------------------------------------------------
// 5. /status reports robot_connected = false when last update is stale
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_status_robot_disconnected_when_stale() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    // Set last update to 60 seconds ago (stale threshold is 30s)
    {
        let mut last_update = app.state.robot_state.last_state_update.write().await;
        *last_update = Some(chrono::Utc::now() - chrono::Duration::seconds(60));
    }

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/status")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status["robotConnected"], false);
}

// ---------------------------------------------------------------------------
// 6. check_robot_connection reports stale when robot hasn't sent updates
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_check_robot_connection_detects_stale() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    // Set a robot URL but make the last update stale
    {
        let mut url = app.state.robot_state.robot_url.write().await;
        *url = Some("http://10.0.0.99:8080".to_string());
    }
    {
        let mut last_update = app.state.robot_state.last_state_update.write().await;
        *last_update = Some(chrono::Utc::now() - chrono::Duration::seconds(60));
    }

    let auth = auth_header("Operator");
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/robot/check")
                .method("GET")
                .header("Authorization", &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(result["connected"], false);
    assert_eq!(result["status"], "error");
}

// ---------------------------------------------------------------------------
// 7. Expired lock allows another user to acquire lock
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_expired_lock_allows_new_acquisition() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    let old_user_id = uuid::Uuid::new_v4();

    // Set an expired lock for a different user
    {
        let mut lock = app.state.robot_state.manual_lock.write().await;
        *lock = Some(backend::robot::state::LockInfo {
            holder_id: old_user_id,
            holder_name: "Old User".to_string(),
            expires_at: chrono::Utc::now() - chrono::Duration::seconds(5),
        });
    }

    let auth = auth_header_for(
        "11111111-1111-1111-1111-111111111111",
        "New User",
        "Operator",
    );

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/drive/lock")
                .method("POST")
                .header("Authorization", &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(
        result["status"], "success",
        "Should be able to acquire lock when previous one is expired, got: {result:?}"
    );

    // Verify the new user holds the lock
    let lock = app.state.robot_state.manual_lock.read().await;
    assert_eq!(lock.as_ref().unwrap().holder_name, "New User");
}

// ---------------------------------------------------------------------------
// 8. Lock renewal (re-acquire by same user) extends the lock
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_lock_renewal_extends_expiry() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    let auth = auth_header_for(
        "22222222-2222-2222-2222-222222222222",
        "Lock User",
        "Operator",
    );

    // Acquire lock
    let _ = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/drive/lock")
                .method("POST")
                .header("Authorization", &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let first_expiry = {
        let lock = app.state.robot_state.manual_lock.read().await;
        lock.as_ref().unwrap().expires_at
    };

    // Wait a tiny bit and renew
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let _ = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/drive/lock")
                .method("POST")
                .header("Authorization", &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let second_expiry = {
        let lock = app.state.robot_state.manual_lock.read().await;
        lock.as_ref().unwrap().expires_at
    };

    assert!(
        second_expiry > first_expiry,
        "Lock renewal should extend the expiry time"
    );
}

// ---------------------------------------------------------------------------
// 9. Process queue skips when lock is active (not expired)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_process_queue_skipped_with_active_lock() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    // Make robot connected and IDLE
    {
        let mut last_update = app.state.robot_state.last_state_update.write().await;
        *last_update = Some(chrono::Utc::now());
    }
    {
        let mut state = app.state.robot_state.current_state.write().await;
        *state = Some(backend::robot::models::RobotState {
            system_health: "OK".to_string(),
            battery_level: 100,
            drive_mode: "IDLE".to_string(),
            cargo_status: "EMPTY".to_string(),
            current_position: "kitchen".to_string(),
            last_node: None,
            target_node: None,
        });
    }

    // Add a route to the queue
    {
        let mut queue = app.state.robot_state.queue.write().await;
        queue.push_back(backend::robot::models::QueuedRoute {
            id: uuid::Uuid::new_v4(),
            start: "A".to_string(),
            destination: "B".to_string(),
            added_at: chrono::Utc::now(),
            added_by: "test".to_string(),
        });
    }

    // Set an active (non-expired) lock
    {
        let mut lock = app.state.robot_state.manual_lock.write().await;
        *lock = Some(backend::robot::state::LockInfo {
            holder_id: uuid::Uuid::new_v4(),
            holder_name: "Lock Holder".to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
        });
    }

    // Subscribe before processing to check if any command is sent
    let mut rx = app.state.robot_state.command_sender.subscribe();

    // Process queue — should NOT dispatch because lock is active
    backend::robot::process_queue(&app.state).await;

    // Queue should still have the route
    let queue = app.state.robot_state.queue.read().await;
    assert_eq!(
        queue.len(),
        1,
        "Queue should not be processed while lock is active"
    );

    // No command should have been sent
    assert!(
        rx.try_recv().is_err(),
        "No command should be dispatched while lock is active"
    );
}

// ---------------------------------------------------------------------------
// 10. Process queue proceeds when lock is expired
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_process_queue_proceeds_with_expired_lock() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    // Make robot connected and IDLE
    {
        let mut last_update = app.state.robot_state.last_state_update.write().await;
        *last_update = Some(chrono::Utc::now());
    }
    {
        let mut state = app.state.robot_state.current_state.write().await;
        *state = Some(backend::robot::models::RobotState {
            system_health: "OK".to_string(),
            battery_level: 100,
            drive_mode: "IDLE".to_string(),
            cargo_status: "EMPTY".to_string(),
            current_position: "kitchen".to_string(),
            last_node: None,
            target_node: None,
        });
    }

    // Subscribe before queueing
    let mut rx = app.state.robot_state.command_sender.subscribe();

    // Add a route to the queue
    {
        let mut queue = app.state.robot_state.queue.write().await;
        queue.push_back(backend::robot::models::QueuedRoute {
            id: uuid::Uuid::new_v4(),
            start: "A".to_string(),
            destination: "B".to_string(),
            added_at: chrono::Utc::now(),
            added_by: "test".to_string(),
        });
    }

    // Set an expired lock
    {
        let mut lock = app.state.robot_state.manual_lock.write().await;
        *lock = Some(backend::robot::state::LockInfo {
            holder_id: uuid::Uuid::new_v4(),
            holder_name: "Old User".to_string(),
            expires_at: chrono::Utc::now() - chrono::Duration::seconds(10),
        });
    }

    // Process queue — should proceed because lock is expired
    backend::robot::process_queue(&app.state).await;

    // Queue should be empty now
    let queue = app.state.robot_state.queue.read().await;
    assert_eq!(
        queue.len(),
        0,
        "Queue should be drained after processing with expired lock"
    );

    // A Navigate command should have been sent
    let cmd = rx
        .try_recv()
        .expect("A command should have been dispatched");
    match cmd {
        backend::robot::models::RobotCommand::Navigate { start, destination } => {
            assert_eq!(start, "A");
            assert_eq!(destination, "B");
        }
        other => panic!("Expected Navigate command, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 11. Process queue skips when robot is stale/disconnected
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_process_queue_skipped_when_robot_stale() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    // Robot is stale (last update 60s ago)
    {
        let mut last_update = app.state.robot_state.last_state_update.write().await;
        *last_update = Some(chrono::Utc::now() - chrono::Duration::seconds(60));
    }
    {
        let mut state = app.state.robot_state.current_state.write().await;
        *state = Some(backend::robot::models::RobotState {
            system_health: "OK".to_string(),
            battery_level: 100,
            drive_mode: "IDLE".to_string(),
            cargo_status: "EMPTY".to_string(),
            current_position: "kitchen".to_string(),
            last_node: None,
            target_node: None,
        });
    }

    // Add a route
    {
        let mut queue = app.state.robot_state.queue.write().await;
        queue.push_back(backend::robot::models::QueuedRoute {
            id: uuid::Uuid::new_v4(),
            start: "X".to_string(),
            destination: "Y".to_string(),
            added_at: chrono::Utc::now(),
            added_by: "test".to_string(),
        });
    }

    backend::robot::process_queue(&app.state).await;

    // Queue should NOT have been processed
    let queue = app.state.robot_state.queue.read().await;
    assert_eq!(
        queue.len(),
        1,
        "Queue should not be processed when robot is stale/disconnected"
    );
}

// ---------------------------------------------------------------------------
// 12. clear_expired_lock clears expired lock and returns true
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_clear_expired_lock() {
    let robot_state = backend::SharedRobotState::new();

    // Set an expired lock
    {
        let mut lock = robot_state.manual_lock.write().await;
        *lock = Some(backend::robot::state::LockInfo {
            holder_id: uuid::Uuid::new_v4(),
            holder_name: "Expired User".to_string(),
            expires_at: chrono::Utc::now() - chrono::Duration::seconds(5),
        });
    }

    let cleared = robot_state.clear_expired_lock().await;
    assert!(
        cleared,
        "Should return true when an expired lock is cleared"
    );

    let lock = robot_state.manual_lock.read().await;
    assert!(lock.is_none(), "Lock should be None after clearing");
}

// ---------------------------------------------------------------------------
// 13. clear_expired_lock does NOT clear active lock
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_clear_expired_lock_preserves_active() {
    let robot_state = backend::SharedRobotState::new();

    {
        let mut lock = robot_state.manual_lock.write().await;
        *lock = Some(backend::robot::state::LockInfo {
            holder_id: uuid::Uuid::new_v4(),
            holder_name: "Active User".to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
        });
    }

    let cleared = robot_state.clear_expired_lock().await;
    assert!(!cleared, "Should return false when lock is still active");

    let lock = robot_state.manual_lock.read().await;
    assert!(lock.is_some(), "Active lock should be preserved");
}

// ---------------------------------------------------------------------------
// 14. is_robot_connected returns false when no updates
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_is_robot_connected_false_when_no_updates() {
    let robot_state = backend::SharedRobotState::new();
    assert!(!robot_state.is_robot_connected().await);
}

// ---------------------------------------------------------------------------
// 15. is_robot_connected returns true after fresh update
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_is_robot_connected_true_after_fresh_update() {
    let robot_state = backend::SharedRobotState::new();
    {
        let mut last_update = robot_state.last_state_update.write().await;
        *last_update = Some(chrono::Utc::now());
    }
    assert!(robot_state.is_robot_connected().await);
}

// ---------------------------------------------------------------------------
// 16. is_robot_connected returns false when stale
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_is_robot_connected_false_when_stale() {
    let robot_state = backend::SharedRobotState::new();
    {
        let mut last_update = robot_state.last_state_update.write().await;
        *last_update = Some(chrono::Utc::now() - chrono::Duration::seconds(60));
    }
    assert!(!robot_state.is_robot_connected().await);
}

// ---------------------------------------------------------------------------
// 17. Robot state update records last_state_update timestamp
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_robot_state_update_records_timestamp() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    // Verify no timestamp initially
    {
        let last_update = app.state.robot_state.last_state_update.read().await;
        assert!(last_update.is_none());
    }

    let payload = serde_json::json!({
        "systemHealth": "OK",
        "batteryLevel": 80,
        "driveMode": "IDLE",
        "cargoStatus": "EMPTY",
        "currentPosition": "lobby",
        "lastNode": null,
        "targetNode": null
    });

    let before = chrono::Utc::now();

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/table/state")
                .method("POST")
                .header("Content-Type", "application/json")
                .header("X-Api-Key", "test_robot_api_key")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let after = chrono::Utc::now();

    assert_eq!(response.status(), StatusCode::OK);

    let last_update = app.state.robot_state.last_state_update.read().await;
    let ts = last_update.expect("last_state_update should be set after state update");
    assert!(ts >= before && ts <= after, "Timestamp should be recent");
}

// ---------------------------------------------------------------------------
// 18. Active route clears when robot reports IDLE
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_active_route_clears_on_idle() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    // Set an active route
    {
        let mut active = app.state.robot_state.active_route.write().await;
        *active = Some(backend::robot::models::QueuedRoute {
            id: uuid::Uuid::new_v4(),
            start: "A".to_string(),
            destination: "B".to_string(),
            added_at: chrono::Utc::now(),
            added_by: "test".to_string(),
        });
    }

    // Robot reports IDLE
    let payload = serde_json::json!({
        "systemHealth": "OK",
        "batteryLevel": 75,
        "driveMode": "IDLE",
        "cargoStatus": "EMPTY",
        "currentPosition": "B",
        "lastNode": "A",
        "targetNode": "B"
    });

    let _ = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/table/state")
                .method("POST")
                .header("Content-Type", "application/json")
                .header("X-Api-Key", "test_robot_api_key")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let active = app.state.robot_state.active_route.read().await;
    assert!(
        active.is_none(),
        "Active route should be cleared when robot reports IDLE"
    );
}

// ---------------------------------------------------------------------------
// 19. Active route persists when robot is NOT idle
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_active_route_persists_when_driving() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    {
        let mut active = app.state.robot_state.active_route.write().await;
        *active = Some(backend::robot::models::QueuedRoute {
            id: uuid::Uuid::new_v4(),
            start: "A".to_string(),
            destination: "B".to_string(),
            added_at: chrono::Utc::now(),
            added_by: "test".to_string(),
        });
    }

    // Robot reports NAVIGATING (not IDLE)
    let payload = serde_json::json!({
        "systemHealth": "OK",
        "batteryLevel": 75,
        "driveMode": "NAVIGATING",
        "cargoStatus": "EMPTY",
        "currentPosition": "hallway",
        "lastNode": "A",
        "targetNode": "B"
    });

    let _ = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/table/state")
                .method("POST")
                .header("Content-Type", "application/json")
                .header("X-Api-Key", "test_robot_api_key")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let active = app.state.robot_state.active_route.read().await;
    assert!(
        active.is_some(),
        "Active route should persist when robot is still driving"
    );
}

// ---------------------------------------------------------------------------
// 20. Viewer cannot acquire lock
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_viewer_cannot_acquire_lock() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    let auth = auth_header("Viewer");

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/drive/lock")
                .method("POST")
                .header("Authorization", &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// 21. select_route blocked when lock is active
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_select_route_blocked_by_active_lock() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    // Set active lock
    {
        let mut lock = app.state.robot_state.manual_lock.write().await;
        *lock = Some(backend::robot::state::LockInfo {
            holder_id: uuid::Uuid::new_v4(),
            holder_name: "Locker".to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
        });
    }

    let auth = auth_header_for(
        "33333333-3333-3333-3333-333333333333",
        "Route User",
        "Operator",
    );

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/routes/select")
                .method("POST")
                .header("Authorization", &auth)
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({"start": "A", "destination": "B"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(result["status"], "error");
    assert!(result["message"].as_str().unwrap().contains("locked"));
}

// ---------------------------------------------------------------------------
// 22. Only lock holder can release lock
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_only_holder_can_release_lock() {
    let app = match common::setup_test_app().await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    let holder_id = "44444444-4444-4444-4444-444444444444";

    // Set lock for the holder
    {
        let mut lock = app.state.robot_state.manual_lock.write().await;
        *lock = Some(backend::robot::state::LockInfo {
            holder_id: uuid::Uuid::parse_str(holder_id).unwrap(),
            holder_name: "Holder".to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
        });
    }

    // Another user tries to release
    let other_auth = auth_header_for(
        "55555555-5555-5555-5555-555555555555",
        "Other User",
        "Operator",
    );

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/drive/lock")
                .method("DELETE")
                .header("Authorization", &other_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        result["status"], "error",
        "Non-holder should not be able to release lock"
    );

    // Verify lock is still held
    let lock = app.state.robot_state.manual_lock.read().await;
    assert!(lock.is_some(), "Lock should still be held");
}
