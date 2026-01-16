# Auth API

This document describes the **auth-related HTTP API** implemented in the backend.

## Quick reference

| Method | Path        | Auth                 | Purpose                                      |
| ------ | ----------- | -------------------- | -------------------------------------------- |
| POST   | `/register` | Public               | Create a new user account                    |
| POST   | `/login`    | Public               | Authenticate and receive a JWT               |
| GET    | `/me`       | JWT (Bearer)         | Fetch the authenticated user                 |
| GET    | `/user`     | JWT (Bearer) + Admin | List users or fetch a specific user by `id`  |
| POST   | `/user`     | JWT (Bearer) + Admin | Update user fields (`name`, `email`, `role`) |
| DELETE | `/user`     | JWT (Bearer) + Admin | Delete a user                                |

## Authentication model

- **JWT Bearer tokens** are issued by `POST /login`.
- Authenticated endpoints require the header:
  - `Authorization: Bearer <jwt>`
- The backend verifies the token using `JWT_SECRET` (HMAC; jsonwebtoken defaults) and validates expiry (`exp`).

### JWT claims

Tokens contain these claims (stored in request extensions by middleware):

```json
{
  "sub": "<user uuid>",
  "name": "<user name>",
  "role": "user|admin",
  "exp": 1730000000
}
```

### Common auth errors (middleware)

For routes protected by auth middleware:

- Missing `Authorization` header → `401` with `{"error":"Missing authorization header"}`
- Non-UTF8 `Authorization` header → `401` with `{"error":"Invalid authorization header"}`
- Not prefixed with `Bearer ` → `401` with `{"error":"Invalid authorization header format"}`
- Invalid/expired token → `401` with `{"error":"Invalid or expired token"}`

### Admin authorization

Admin routes require `claims.role == "admin"`.

- Not authenticated / claims missing → `401` with `{"error":"No authentication information found"}`
- Authenticated but not admin → `403` with `{"error":"Admin access required"}`

---

## `POST /register`

Create a new user account.

### Request

- Body (JSON):

```json
{
  "name": "Jane Doe",
  "email": "jane@example.com",
  "password": "plaintext password"
}
```

### Responses

- `201 Created` with newly created user (password hash is never returned):

```json
{
  "id": "<uuid>",
  "name": "Jane Doe",
  "email": "jane@example.com",
  "role": "user"
}
```

### Error cases

- `400 Bad Request` if email already exists:

```json
{ "error": "User with this email already exists" }
```

- `500 Internal Server Error` on DB errors, or bcrypt hashing errors, with an `error` string.

---

## `POST /login`

Authenticate by email + password and receive a JWT.

### Request

- Body (JSON):

```json
{
  "email": "jane@example.com",
  "password": "plaintext password"
}
```

### Responses

- `200 OK`:

```json
{ "token": "<jwt>" }
```

### Error cases

- `401 Unauthorized` if user does not exist **or** password is incorrect:

```json
{ "error": "Invalid credentials" }
```

- `500 Internal Server Error` on DB errors, password verification errors, or token generation errors.

---

## `GET /me` (authenticated)

Fetch the authenticated user’s current data.

### Auth

- Requires `Authorization: Bearer <jwt>`.

### Responses

- `200 OK`:

```json
{
  "id": "<uuid>",
  "name": "Jane Doe",
  "email": "jane@example.com",
  "role": "user"
}
```

### Error cases

- `400 Bad Request` if the token’s `sub` is not a valid UUID:

```json
{ "error": "Invalid user ID" }
```

- `500 Internal Server Error` on DB errors.

---

## Admin endpoints (require authenticated admin)

All endpoints below require:

- `Authorization: Bearer <jwt>`
- `role` claim equals `admin`

### `GET /user`

Fetch users.

#### Request

- Optional query parameter: `id=<uuid>`

#### Responses

- `200 OK` with **one user** when `id` is provided:

```json
{
  "id": "<uuid>",
  "name": "Jane Doe",
  "email": "jane@example.com",
  "role": "user"
}
```

- `200 OK` with **an array of users** when `id` is omitted:

```json
[{ "id": "<uuid>", "name": "...", "email": "...", "role": "..." }]
```

#### Error cases

- `404 Not Found` when `id` is provided but user doesn’t exist:

```json
{ "error": "User not found" }
```

- `500 Internal Server Error` on DB errors.

### `POST /user`

Update a user’s `name`, `email`, and/or `role`.

#### Request

- Body (JSON):

```json
{
  "id": "<uuid>",
  "name": "New Name",
  "email": "new@example.com",
  "role": "admin"
}
```

All of `name`, `email`, `role` are optional; `id` is required.

#### Responses

- `200 OK` with updated user:

```json
{
  "id": "<uuid>",
  "name": "New Name",
  "email": "new@example.com",
  "role": "admin"
}
```

#### Error cases

- `404 Not Found` if `id` doesn’t exist.
- `500 Internal Server Error` on DB errors.

### `DELETE /user`

Delete a user.

#### Request

- Body (JSON):

```json
{ "id": "<uuid>" }
```

#### Responses

- `204 No Content` on success.

#### Error cases

- `404 Not Found` if user doesn’t exist.
- `500 Internal Server Error` on DB errors.
