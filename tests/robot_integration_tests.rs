use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use uuid::Uuid;

mod common;

async fn insert_test_user(
    app: &common::TestApp,
    user_id: Uuid,
    role: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO users (id, name, email, password_hash, role, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(user_id)
    .bind(format!("{role} User"))
    .bind(format!("{}@example.com", role.to_ascii_lowercase()))
    .bind("hashed_password")
    .bind(role)
    .bind(Utc::now())
    .execute(&app.db)
    .await
    .map(|_| ())
}

#[tokio::test]
async fn test_get_nodes_proxies_to_robot() {
    // 1. Setup
    let app = match common::setup_test_app().await {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Skipping test_get_nodes_proxies_to_robot: {e}");
            return;
        }
    };

    // Mock Key for this to work - we need to authenticate as user first
    // Skipping user auth setup for brevity or adding a helper?
    // The route /nodes is protected.
    // Let's create a helper "create_authenticated_request".
    // Alternatively, I'll allow /nodes to be public? No, it's protected.

    // Stub auth: We can mock the JWT secret or just create a valid token since we have the secret.
    // The test app uses "test_secret".

    let token =
        backend::auth::security::create_jwt("user_id", "Test User", "user", "test_secret", 1)
            .unwrap();
    let auth_header = format!("Bearer {token}");

    // Mock Robot Server
    let mock_server = MockServer::start().await;

    // Update app state to point to mock robot
    {
        let mut url_lock = app.state.robot_state.robot_url.write().await;
        *url_lock = Some(mock_server.uri());
    }

    Mock::given(method("GET"))
        .and(path("/nodes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
             "nodes": ["Kitchen", "LivingRoom"]
        })))
        .mount(&mock_server)
        .await;

    // 2. Execute
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/nodes")
                .method("GET")
                .header("Authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 3. Assert
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let nodes: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(nodes["nodes"][0], "Kitchen");
    assert_eq!(nodes["nodes"][1], "LivingRoom");
}

#[tokio::test]
async fn test_update_robot_state_webhook() {
    let app = match common::setup_test_app().await {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Skipping test_update_robot_state_webhook: {e}");
            return;
        }
    };

    // 1. Prepare payload
    let payload = serde_json::json!({
        "systemHealth": "OK",
        "batteryLevel": 85,
        "driveMode": "IDLE",
        "cargoStatus": "EMPTY",
        "currentPosition": "hallway",
        "lastNode": "kitchen",
        "targetNode": "pharmacy",

    });

    // 2. Execute
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

    assert_eq!(response.status(), StatusCode::OK);

    // 3. Verify state in memory
    let current_state = app.state.robot_state.current_state.read().await;
    assert!(current_state.is_some());
    let s = current_state.as_ref().unwrap();
    assert_eq!(s.battery_level, 85);
}

#[tokio::test]
async fn test_get_robot_debug_requires_admin_and_merges_robot_status() {
    let app = match common::setup_test_app().await {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Skipping test_get_robot_debug_requires_admin_and_merges_robot_status: {e}");
            return;
        }
    };

    let admin_id = Uuid::new_v4();
    let viewer_id = Uuid::new_v4();
    insert_test_user(&app, admin_id, "Admin").await.unwrap();
    insert_test_user(&app, viewer_id, "Viewer").await.unwrap();

    let admin_token = backend::auth::security::create_jwt(
        &admin_id.to_string(),
        "Admin User",
        "Admin",
        "test_secret",
        1,
    )
    .unwrap();
    let viewer_token = backend::auth::security::create_jwt(
        &viewer_id.to_string(),
        "Viewer User",
        "Viewer",
        "test_secret",
        1,
    )
    .unwrap();

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sensors": {
                "ir": { "left": true, "middle": false, "right": true },
                "light": { "luxValid": true, "lux": 124.5 },
                "power": { "valid": true, "batteryVoltage": 12.4, "currentA": 1.6, "powerW": 19.8 }
            }
        })))
        .mount(&mock_server)
        .await;

    {
        let mut robot_url = app.state.robot_state.robot_url.write().await;
        *robot_url = Some(mock_server.uri());
    }
    {
        let mut current_state = app.state.robot_state.current_state.write().await;
        *current_state = Some(backend::robot::models::RobotState {
            system_health: "OK".to_string(),
            battery_level: 92,
            drive_mode: "AUTO".to_string(),
            cargo_status: "EMPTY".to_string(),
            current_position: "Hallway".to_string(),
            last_node: Some("Kitchen".to_string()),
            target_node: Some("Lab".to_string()),
            gyroscope: None,
            last_read_uuid: None,
            lux: None,
            infrared: None,
            voltage_v: None,
            current_a: None,
            power_w: None,
        });
    }
    {
        let mut last_update = app.state.robot_state.last_state_update.write().await;
        *last_update = Some(Utc::now());
    }

    let viewer_response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/robot/debug")
                .method("GET")
                .header("Authorization", format!("Bearer {viewer_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(viewer_response.status(), StatusCode::FORBIDDEN);

    let admin_response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/robot/debug")
                .method("GET")
                .header("Authorization", format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(admin_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let debug: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(debug["telemetry"]["systemHealth"], "OK");
    assert_eq!(debug["connection"]["robotStatusReachable"], true);
    assert_eq!(debug["sensors"]["light"]["lux"], 124.5);
    assert_eq!(debug["sensors"]["infrared"]["front"], false);
    assert_eq!(debug["sensors"]["infrared"]["left"], true);
    assert_eq!(debug["sensors"]["power"]["voltageV"], 12.4);
    assert_eq!(
        debug["sensors"]["gyroscope"]["source"],
        "unavailable_until_firmware"
    );
    assert!(debug["sensors"]["gyroscope"]["xDps"].is_null());
    assert_eq!(
        debug["sensors"]["rfid"]["source"],
        "unavailable_until_firmware"
    );
    assert!(debug["sensors"]["rfid"]["lastReadUuid"].is_null());
}

#[tokio::test]
async fn test_get_robot_debug_degrades_when_robot_status_unreachable() {
    let app = match common::setup_test_app().await {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Skipping test_get_robot_debug_degrades_when_robot_status_unreachable: {e}");
            return;
        }
    };

    let admin_id = Uuid::new_v4();
    insert_test_user(&app, admin_id, "Admin").await.unwrap();

    let admin_token = backend::auth::security::create_jwt(
        &admin_id.to_string(),
        "Admin User",
        "Admin",
        "test_secret",
        1,
    )
    .unwrap();

    {
        let mut robot_url = app.state.robot_state.robot_url.write().await;
        *robot_url = Some("http://127.0.0.1:9".to_string());
    }
    {
        let mut current_state = app.state.robot_state.current_state.write().await;
        *current_state = Some(backend::robot::models::RobotState {
            system_health: "OK".to_string(),
            battery_level: 76,
            drive_mode: "IDLE".to_string(),
            cargo_status: "EMPTY".to_string(),
            current_position: "Dock".to_string(),
            last_node: None,
            target_node: None,
            gyroscope: None,
            last_read_uuid: None,
            lux: Some(88.0),
            infrared: Some(backend::robot::models::RobotInfraredReading {
                front: Some(true),
                left: Some(false),
                right: Some(true),
            }),
            voltage_v: Some(11.9),
            current_a: Some(0.8),
            power_w: Some(9.5),
        });
    }

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/robot/debug")
                .method("GET")
                .header("Authorization", format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let debug: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(debug["connection"]["robotStatusReachable"], false);
    assert_eq!(debug["sensors"]["light"]["lux"], 88.0);
    assert_eq!(debug["sensors"]["light"]["source"], "table_state");
    assert_eq!(debug["sensors"]["infrared"]["front"], true);
    assert_eq!(debug["sensors"]["power"]["currentA"], 0.8);
}
