use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde_json::json;
use std::sync::Arc;

use crate::auth::models::Claims;
use crate::auth::roles;
use crate::AppState;

pub fn hash_password(password: &str) -> Result<String, bcrypt::BcryptError> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST)
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, bcrypt::BcryptError> {
    bcrypt::verify(password, hash)
}

pub fn create_jwt(
    user_id: &str,
    name: &str,
    role: &str,
    secret: &str,
    expiry_hours: i64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(expiry_hours))
        .expect("valid timestamp")
        .timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        name: name.to_string(),
        role: role.to_string(),
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn decode_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;

    Ok(token_data.claims)
}

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    #[allow(unused_mut)] mut req: Request,
    next: Next,
) -> Result<Response, impl IntoResponse> {
    let auth_header = req.headers().get(header::AUTHORIZATION);

    let auth_header = match auth_header {
        Some(header) => header.to_str().map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid authorization header"})),
            )
        })?,
        None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Missing authorization header"})),
            ));
        }
    };

    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid authorization header format"})),
        )
    })?;

    let claims = decode_jwt(token, &state.config.jwt_secret).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid or expired token"})),
        )
    })?;

    req.extensions_mut().insert(claims);

    Ok(next.run(req).await)
}

pub async fn admin_middleware(req: Request, next: Next) -> Result<Response, impl IntoResponse> {
    let claims = req.extensions().get::<Claims>().ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "No authentication information found"})),
        )
    })?;

    if !roles::is_admin(&claims.role) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Admin access required"})),
        ));
    }

    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_hashing_and_verification() {
        let password = "my_secure_password";
        let hash = hash_password(password).expect("hashing failed");

        assert_ne!(password, hash);
        assert!(verify_password(password, &hash).expect("verification failed"));
        assert!(!verify_password("wrong_password", &hash).expect("verification failed"));
    }

    #[test]
    fn test_jwt_creation_and_decoding() {
        let secret = "super_secret_key";
        let user_id = "123-456";
        let name = "Test User";
        let role = "Viewer";
        let expiry = 1;

        let token = create_jwt(user_id, name, role, secret, expiry).expect("creation failed");
        let claims = decode_jwt(&token, secret).expect("decoding failed");

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.name, name);
        assert_eq!(claims.role, role);
    }

    #[test]
    fn test_jwt_expiration_validation() {
        let secret = "super_secret_key";
        // Create a JWT manually with past expiration
        let claims = Claims {
            sub: "123".to_string(),
            name: "test".to_string(),
            role: "Viewer".to_string(),
            exp: (chrono::Utc::now().timestamp() - 3600) as usize, // 1 hour ago
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        let result = decode_jwt(&token, secret);
        assert!(result.is_err());
    }
}
