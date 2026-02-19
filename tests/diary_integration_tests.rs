use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use backend::{
    auth::models::{LoginRequest, LoginResponse, RegisterRequest},
    diary::models::{CreateDiaryRequest, DeleteDiaryRequest, DiaryResponse},
};
use tower::ServiceExt;

mod common;
use common::TestApp;

async fn get_auth_token(app: &TestApp) -> String {
    // Register
    let _ = app
        .router
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

    // Promote to Operator so writes are allowed
    sqlx::query("UPDATE users SET role = 'Operator' WHERE email = $1")
        .bind("diary@example.com")
        .execute(&app.db)
        .await
        .unwrap();

    // Login
    let response = app
        .router
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

async fn get_viewer_token(app: &TestApp) -> String {
    // Register as viewer
    let _ = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/register")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&RegisterRequest {
                        name: "Viewer User".into(),
                        email: "viewer@example.com".into(),
                        password: "password".into(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Login as Viewer (default role - no promotion)
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/login")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&LoginRequest {
                        email: "viewer@example.com".into(),
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

#[tokio::test]
async fn test_diary_crud() {
    let app = match common::setup_test_app().await {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Skipping test_diary_crud: {e}");
            return;
        }
    };
    let token = get_auth_token(&app).await;
    let auth_header = format!("Bearer {token}");

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

#[tokio::test]
async fn test_viewer_cannot_create_or_update_diary() {
    let app = match common::setup_test_app().await {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Skipping test_viewer_cannot_create_or_update_diary: {e}");
            return;
        }
    };

    // Register + login → Viewer token
    let token = get_viewer_token(&app).await;
    let auth_header = format!("Bearer {token}");

    let create_payload = CreateDiaryRequest {
        id: None,
        working_minutes: 30,
        text: "Viewer write attempt".into(),
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

    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Attempt update should also be forbidden
    let update_payload = CreateDiaryRequest {
        id: Some(uuid::Uuid::new_v4()),
        working_minutes: 45,
        text: "Viewer update attempt".into(),
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

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
