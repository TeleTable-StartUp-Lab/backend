use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use sqlx::{Pool, Postgres};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;

#[sqlx::test]
async fn test_get_nodes_proxies_to_robot(pool: Pool<Postgres>) {
    // 1. Setup
    let app = common::spawn_app(pool).await;

    // Mock Key for this to work - we need to authenticate as user first
    // Skipping user auth setup for brevity or adding a helper?
    // The route /nodes is protected.
    // Let's create a helper "create_authenticated_request".
    // Alternatively, I'll allow /nodes to be public? No, it's protected.

    // Stub auth: We can mock the JWT secret or just create a valid token since we have the secret.
    // The test app uses "test_secret".

    let token =
        backend::auth::auth::create_jwt("user_id", "Test User", "user", "test_secret", 1).unwrap();
    let auth_header = format!("Bearer {}", token);

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

#[sqlx::test]
async fn test_update_robot_state_webhook(pool: Pool<Postgres>) {
    let app = common::spawn_app(pool).await;

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
