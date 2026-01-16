use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use backend::{
    auth::models::{LoginRequest, LoginResponse, RegisterRequest},
    diary::models::{CreateDiaryRequest, DeleteDiaryRequest, DiaryResponse},
};
use sqlx::{Pool, Postgres};
use tower::ServiceExt;

mod common;

async fn get_auth_token(app: &axum::Router) -> String {
    // Register
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/register")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&RegisterRequest {
                        name: "Diary User".into(),
                        email: "diary@example.com".into(),
                        password: "password".into(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Login
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/login")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&LoginRequest {
                        email: "diary@example.com".into(),
                        password: "password".into(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_resp: LoginResponse = serde_json::from_slice(&body).unwrap();
    login_resp.token
}

#[sqlx::test]
async fn test_diary_crud(pool: Pool<Postgres>) {
    let app = common::spawn_app(pool).await;
    let token = get_auth_token(&app.router).await;
    let auth_header = format!("Bearer {}", token);

    // 1. Create Diary Entry
    let create_payload = CreateDiaryRequest {
        id: None,
        working_minutes: 120,
        text: "Worked on robot".into(),
    };

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/diary")
                .method("POST")
                .header("Content-Type", "application/json")
                .header("Authorization", &auth_header)
                .body(Body::from(serde_json::to_string(&create_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created_diary: DiaryResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(created_diary.working_minutes, 120);
    assert_eq!(created_diary.text, "Worked on robot");

    // 2. Get Diary Stream/List (Check if it exists)
    // Note: API is /diary (GET params) or /diary/all ?
    // Check lib.rs routes. /diary with GET calls get_diary.

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/diary?id={}", created_diary.id))
                .method("GET")
                .header("Authorization", &auth_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // 3. Update Diary Entry
    let update_payload = CreateDiaryRequest {
        id: Some(created_diary.id),
        working_minutes: 150,
        text: "Worked on robot longer".into(),
    };

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/diary")
                .method("POST")
                .header("Content-Type", "application/json")
                .header("Authorization", &auth_header)
                .body(Body::from(serde_json::to_string(&update_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // 4. Delete Diary Entry
    let delete_payload = DeleteDiaryRequest {
        id: created_diary.id,
    };
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/diary")
                .method("DELETE")
                .header("Authorization", &auth_header)
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&delete_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}
