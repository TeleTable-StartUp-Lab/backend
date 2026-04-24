# Auth API

This document describes the **auth-related HTTP API** implemented in the backend.

## Quick reference

| Method | Path        | Auth                 | Purpose                                                  |
| ------ | ----------- | -------------------- | -------------------------------------------------------- |
| POST   | `/register` | Public               | Create a new user account                                |
| POST   | `/login`    | Public               | Authenticate and receive a JWT                           |
| GET    | `/me`       | JWT (Bearer)         | Fetch the authenticated user                             |
| GET    | `/users`    | JWT (Bearer) + Admin | List users (admin alias for `/user` without query)       |
| GET    | `/user`     | JWT (Bearer) + Admin | List users or fetch a specific user by `id`              |
| GET    | `/users/{id}/sessions` | JWT (Bearer) + Admin | Fetch full session history for a user |
| GET    | `/user/{id}/sessions`  | JWT (Bearer) + Admin | Backward-compatible alias of `/users/{id}/sessions` |
| POST   | `/user`     | JWT (Bearer) + Admin | Update user fields (`name`, `email`, `role`, `password`) |
| DELETE | `/user`     | JWT (Bearer) + Admin | Delete a user                                            |

## Authentication model

- **JWT Bearer tokens** are issued by `POST /login`.
- Authenticated endpoints require the header:
  - `Authorization: Bearer <jwt>`
- The backend verifies the token using `JWT_SECRET` (HMAC; jsonwebtoken defaults) and validates expiry (`exp`).
- **Real-time role enforcement for HTTP routes:** On every authenticated HTTP request, the auth middleware fetches the user's **current role from the database** (with a Redis user-cache fast path) and overrides the role embedded in the JWT. This ensures role changes (e.g. Admin demoting an Operator to Viewer) take effect immediately for Bearer-token HTTP endpoints — the user does not need to log out and back in.
- **WebSocket auth differs:** `/ws/drive/manual?token=<jwt>` and `/ws/robot/events?token=<jwt>` decode the JWT from the query token directly and do **not** run the HTTP auth middleware, so they do not refresh the role from the database. Their authorization depends on the role embedded in the presented token.
- **JWT cache invalidation on role change:** When an admin updates a user via `POST /user`, all cached JWT validation entries for that user are invalidated in Redis, forcing a fresh token decode and role lookup on the next request.

## Roles and permissions

The backend uses three roles: **Admin**, **Operator**, and **Viewer**. Expected capabilities are below (enforcement gaps are noted).

| Capability                                                                         | Admin                     | Operator        | Viewer          |
| ---------------------------------------------------------------------------------- | ------------------------- | --------------- | --------------- |
| Manage users (`/user` list/update/delete)                                          | Yes                       | No              | No              |
| Create/update diary entries (`POST /diary`)                                        | Yes                       | Yes             | No (403)        |
| Delete diary entries (`DELETE /diary`)                                             | Yes                       | Yes             | No (403)        |
| Read own diary entries (`GET /diary`)                                              | Yes                       | Yes             | Yes             |
| Read all diaries (`GET /diary/all`)                                                | Public endpoint (no auth) | Public endpoint | Public endpoint |
| Select robot routes (`POST /routes/select`)                                        | Yes                       | Yes             | No              |
| Acquire/release manual drive lock (`POST/DELETE /drive/lock`)                      | Yes                       | Yes             | No              |
| Manage route queue (`POST /routes`, `DELETE /routes/:id`, `POST /routes/optimize`) | Yes                       | No              | No              |
| Read robot nodes/live status (`GET /nodes`, `GET /robot/notifications`, `GET /ws/robot/events?token=<jwt>`) | Yes | Yes | Yes |
| Read admin debug snapshot (`GET /robot/debug`) | Yes | No | No |

### JWT claims

Tokens contain these claims (stored in request extensions by middleware):

```json
{
  "sub": "<user uuid>",
  "name": "<user name>",
  "role": "Admin|Operator|Viewer",
  "exp": 1730000000
}
```

> **Note:** For HTTP routes protected by auth middleware, the `role` claim is set at login time but refreshed from the database before passing claims to handlers. For WebSocket routes that accept `?token=<jwt>`, the embedded JWT role is used as-is for that connection.

### Common auth errors (middleware)

For routes protected by auth middleware:

- Missing `Authorization` header → `401` with `{"error":"Missing authorization header"}`
- Non-UTF8 `Authorization` header → `401` with `{"error":"Invalid authorization header"}`
- Not prefixed with `Bearer ` → `401` with `{"error":"Invalid authorization header format"}`
- Invalid/expired token → `401` with `{"error":"Invalid or expired token"}`

### Admin authorization

Admin routes require `claims.role == "Admin"` (checked against the database-refreshed role, not the raw JWT claim).

- Not authenticated / claims missing → `401` with `{"error":"No authentication information found"}`
- Authenticated but not admin → `403` with `{"error":"Admin access required"}`

### Client metadata and anti-abuse controls

Auth endpoints capture client context and apply abuse controls:

- **Client IP extraction order:** `X-Real-IP` → first entry in `X-Forwarded-For` → socket address (`ConnectInfo`) → `"unknown"`.
- **Session history:** both successful register and login write a new `sessions` row with `ip_address`, `fingerprint_data`, and `user_agent`.
- **Signup IP rate limit:** `POST /register` allows up to **5 attempts per 600 seconds** per IP, then returns `429`.
- **SwiftShader signal timeout:** if fingerprint renderer metadata contains `SwiftShader`, signup is rejected and the IP is timed out for signup for **86400 seconds**.

#### Fingerprint payload details

`fingerprintData` is intentionally open-ended JSON (`serde_json::Value`) and is stored as-is in `sessions.fingerprint_data`.

- The backend does **not** enforce a strict fingerprint schema.
- Any nested objects/arrays are accepted.
- If omitted, the stored value defaults to `{}`.

Typical payloads can include much more than renderer data, for example:

```json
{
  "gpu": {
    "vendor": "Google Inc.",
    "renderer": "ANGLE (NVIDIA, NVIDIA GeForce RTX 3060 Direct3D11 vs_5_0 ps_5_0)",
    "webglVersion": "WebGL 1.0"
  },
  "screen": { "width": 1920, "height": 1080, "colorDepth": 24 },
  "timezone": "Europe/Berlin",
  "language": "en-US",
  "platform": "Win32",
  "hardwareConcurrency": 12,
  "deviceMemory": 8,
  "touchSupport": false,
  "canvasHash": "...",
  "audioHash": "...",
  "fontsHash": "..."
}
```

SwiftShader detection is narrow and explicit:

- The backend recursively scans the fingerprint JSON.
- It only checks string values under keys whose name contains `renderer` (case-insensitive).
- If such a value contains `swiftshader` (case-insensitive), signup is blocked and the IP timeout is applied.

---

## `POST /register`

Create a new user account.

### Request

- Body (JSON):

```json
{
  "name": "Jane Doe",
  "email": "jane@example.com",
  "password": "plaintext password",
  "fingerprintData": {
    "gpu": { "renderer": "ANGLE (SwiftShader Device (Subzero))" }
  }
}
```

`fingerprintData` is optional and can include any nested JSON fields. On signup, renderer-like fields are scanned for `SwiftShader`; matching requests are rejected and the source IP is temporarily timed out.

### Responses

- `201 Created` with newly created user (password hash is never returned):

```json
{
  "id": "<uuid>",
  "name": "Jane Doe",
  "email": "jane@example.com",
  "role": "Viewer",
  "created_at": "2026-03-14T12:34:56Z",
  "last_sign_on": null
}
```

### Error cases

- `400 Bad Request` if required fields are missing/blank:

```json
{ "error": "Email and name are required" }
```

- `400 Bad Request` if email already exists:

```json
{ "error": "User with this email already exists" }
```

- `429 Too Many Requests` if too many registrations come from the same IP in a short window:

```json
{
  "error": "Too many account creation attempts from this IP. Please try again later.",
  "retry_after_seconds": 600
}
```

- `429 Too Many Requests` if a SwiftShader renderer bot-signal is detected in `fingerprintData` (IP is temporarily timed out for signup):

```json
{
  "error": "Account creation is temporarily blocked for this IP.",
  "retry_after_seconds": 86400
}
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
  "password": "plaintext password",
  "fingerprintData": {
    "gpu": { "renderer": "ANGLE (NVIDIA, NVIDIA GeForce RTX ...)" }
  }
}
```

`fingerprintData` is optional, accepts arbitrary nested JSON, and is persisted into `sessions.fingerprint_data` on successful login.

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
  "role": "Viewer",
  "created_at": "2026-03-14T12:34:56Z",
  "last_sign_on": "2026-03-14T13:45:10Z"
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
- effective role resolves to `Admin` (middleware refreshes role from DB each request)

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
  "role": "Viewer",
  "created_at": "2026-03-14T12:34:56Z",
  "last_sign_on": "2026-03-14T13:45:10Z"
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
  "role": "Operator",
  "password": "new_password_if_resetting"
}
```

All of `name`, `email`, `role`, `password` are optional; `id` is required. If `password` is provided, it is bcrypt-hashed before being stored. Empty passwords are rejected with `400`.

> **Side effects:** When a user is updated, the backend invalidates both the **user data cache** and all **cached JWT validations** for that user in Redis. This ensures role changes take effect on the very next request the affected user makes.

#### Responses

- `200 OK` with updated user:

```json
{
  "id": "<uuid>",
  "name": "New Name",
  "email": "new@example.com",
  "role": "Operator",
  "created_at": "2026-03-14T12:34:56Z",
  "last_sign_on": "2026-03-14T13:45:10Z"
}
```

### `GET /users/{id}/sessions` (admin)

Fetches full session history for a user, newest first.

#### Responses

- `200 OK`:

```json
[
  {
    "id": "<session uuid>",
    "user_id": "<user uuid>",
    "ip_address": "203.0.113.7",
    "fingerprint_data": { "gpu": { "renderer": "ANGLE (...)" } },
    "user_agent": "Mozilla/5.0 ...",
    "created_at": "2026-03-14T13:45:10Z"
  }
]
```

#### Error cases

- `404 Not Found` if user doesn’t exist.
- `500 Internal Server Error` on DB errors.

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
