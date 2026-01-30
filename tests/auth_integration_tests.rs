use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use backend::auth::models::{LoginRequest, LoginResponse, RegisterRequest};
use tower::ServiceExt; // for oneshot

mod common;

#[tokio::test]
async fn test_register_and_login_flow() {
    let app = match common::setup_test_app().await {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Skipping test_register_and_login_flow: {e}");
            return;
        }
    };

    // 1. Register
    let register_payload = RegisterRequest {
        name: "Test User".into(),
        email: "test@example.com".into(),
        password: "password123".into(),
    };

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/register")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&register_payload).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    // 2. Login
    let login_payload = LoginRequest {
        email: "test@example.com".into(),
        password: "password123".into(),
    };

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/login")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&login_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_resp: LoginResponse = serde_json::from_slice(&body).unwrap();
    assert!(!login_resp.token.is_empty());

    // 3. Login with wrong password
    let login_payload_bad = LoginRequest {
        email: "test@example.com".into(),
        password: "wrongpassword".into(),
    };

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/login")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&login_payload_bad).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_access_protected_route_without_token() {
    let app = match common::setup_test_app().await {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Skipping test_access_protected_route_without_token: {e}");
            return;
        }
    };

    let response = app
        .router
        .oneshot(
            Request::builder()
                .uri("/me")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
