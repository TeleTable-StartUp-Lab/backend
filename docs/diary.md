# Diary API

This document describes the **diary-related HTTP API** implemented in the backend.

## Quick reference

| Method | Path               | Auth         | Purpose                                                                 |
| ------ | ------------------ | ------------ | ----------------------------------------------------------------------- |
| GET    | `/diary/all`       | Public       | List all diary entries across all users (includes owner name)           |
| POST   | `/diary`           | JWT (Bearer) | Create a new diary entry (no `id`) or update an owned entry (with `id`) |
| GET    | `/diary`           | JWT (Bearer) | List authenticated user’s diary entries                                 |
| GET    | `/diary?id=<uuid>` | JWT (Bearer) | Fetch a specific diary entry (must be owned)                            |
| DELETE | `/diary`           | JWT (Bearer) | Delete a diary entry (must be owned)                                    |

## Data types

### Diary entry (stored)

The backend stores diary entries in PostgreSQL and exposes them via response DTOs.

### `DiaryResponse`

Returned by authenticated diary endpoints:

```json
{
  "id": "<uuid>",
  "owner": "<user uuid>",
  "working_minutes": 60,
  "text": "Did X, Y, Z",
  "created_at": "2026-01-16T10:00:00Z",
  "updated_at": "2026-01-16T12:00:00Z"
}
```

### `DiaryResponseWithUser`

Returned by `GET /diary/all`:

```json
{
  "id": "<uuid>",
  "owner": "<user name>",
  "working_minutes": 60,
  "text": "Did X, Y, Z",
  "created_at": "2026-01-16T10:00:00Z",
  "updated_at": "2026-01-16T12:00:00Z"
}
```

---

## `GET /diary/all` (public)

Return **all diary entries across all users**, newest first.

### Auth

- No authentication required.

### Responses

- `200 OK` with an array of `DiaryResponseWithUser`:

```json
[
  {
    "id": "<uuid>",
    "owner": "Alice",
    "working_minutes": 30,
    "text": "...",
    "created_at": "...",
    "updated_at": "..."
  }
]
```

### Error cases

- `500 Internal Server Error` on DB errors.

---

## Authenticated diary endpoints

All endpoints below require:

- `Authorization: Bearer <jwt>`

Role expectations:

- **Create/update (`POST /diary`)**: Admin and Operator only; Viewer receives `403`.
- **Delete (`DELETE /diary`)**: Admin and Operator only; Viewer receives `403`.
- **Read (`GET /diary`)**: All authenticated roles.

See [docs/auth.md](auth.md) for exact middleware error responses.

### `POST /diary`

Create a new diary entry **or** update an existing one owned by the authenticated user.

#### Request

- Body (JSON):

```json
{
  "id": "<uuid>",
  "working_minutes": 60,
  "text": "Did X, Y, Z"
}
```

- If `id` is omitted or `null`, a new entry is created.
- If `id` is provided, the backend updates that entry **only if it belongs to the authenticated user**.

#### Responses

- `201 Created` when creating a new entry, with `DiaryResponse`.
- `200 OK` when updating an existing entry, with `DiaryResponse`.

#### Error cases

- `400 Bad Request` if the JWT `sub` is not a UUID: `{"error":"Invalid user ID"}`
- `404 Not Found` when updating with an `id` that doesn’t exist for that user: `{"error":"Diary entry not found"}`
- `500 Internal Server Error` on DB errors (returns `{ "error": "<db error string>" }`)

### `GET /diary`

Fetch diary entries for the authenticated user.

#### Request

- Optional query parameter: `id=<uuid>`

#### Responses

- `200 OK` with one `DiaryResponse` if `id` is provided.
- `200 OK` with an array of `DiaryResponse` if `id` is omitted (ordered by `created_at DESC`).

#### Error cases

- `400 Bad Request` if the JWT `sub` is not a UUID.
- `404 Not Found` if `id` is provided but entry doesn’t exist for that user.
- `500 Internal Server Error` on DB errors.

### `DELETE /diary`

Delete a diary entry owned by the authenticated user.

#### Request

- Body (JSON):

```json
{ "id": "<uuid>" }
```

#### Responses

- `204 No Content` on success.

#### Error cases

- `400 Bad Request` if the JWT `sub` is not a UUID.
- `404 Not Found` if the entry doesn’t exist for that user.
- `500 Internal Server Error` on DB errors.
