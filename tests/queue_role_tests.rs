use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use sqlx::{Pool, Postgres};
use tower::ServiceExt;

mod common;

#[sqlx::test]
async fn test_admin_can_manage_queue(pool: Pool<Postgres>) {
    let app = common::spawn_app(pool).await;

    // 1. Create Admin Token
    let admin_token =
        backend::auth::security::create_jwt("admin_id", "Admin User", "Admin", "test_secret", 1)
            .unwrap();
    let auth_header = format!("Bearer {}", admin_token);

    // 2. Add Route to Queue (POST /routes)
    let payload = serde_json::json!({
        "start": "Home",
        "destination": "Kitchen"
    });

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/routes")
                .method("POST")
                .header("Authorization", &auth_header)
                .header("Content-Type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let queued_route: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let route_id = queued_route["id"].as_str().unwrap();

    assert_eq!(queued_route["start"], "Home");
    assert_eq!(queued_route["destination"], "Kitchen");
    assert_eq!(queued_route["added_by"], "Admin User");

    // 3. Verify Queue Contains Route (GET /routes)
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/routes")
                .method("GET")
                .header("Authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let queue: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(queue.as_array().unwrap().len(), 1);
    assert_eq!(queue[0]["id"], route_id);

    // 4. Delete Route (DELETE /routes/:id)
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/routes/{}", route_id))
                .method("DELETE")
                .header("Authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // 5. Verify Queue Empty
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/routes")
                .method("GET")
                .header("Authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let queue: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(queue.as_array().unwrap().len(), 0);
}

#[sqlx::test]
async fn test_operator_cannot_manage_queue(pool: Pool<Postgres>) {
    let app = common::spawn_app(pool).await;

    let operator_token = backend::auth::security::create_jwt(
        "op_id",
        "Operator User",
        "Operator",
        "test_secret",
        1,
    )
    .unwrap();
    let auth_header = format!("Bearer {}", operator_token);

    // Try to Add Route
    let payload = serde_json::json!({
        "start": "Home",
        "destination": "Kitchen"
    });

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/routes")
                .method("POST")
                .header("Authorization", &auth_header)
                .header("Content-Type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
