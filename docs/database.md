# Database

This document describes the **database layer** used by the backend: how the service connects to PostgreSQL, what schema it manages through migrations, and what the important PostgreSQL data types, constraints, and indexes mean in practice.

## Quick reference

| Topic | Value |
| ----- | ----- |
| Primary database | PostgreSQL |
| Rust access layer | `sqlx` + `PgPool` |
| Connection source | `DATABASE_URL` environment variable |
| Pool size | `10` connections in the app, `5` in integration tests |
| Migration source | `./migrations` |
| Main tables | `users`, `diary_entries`, `sessions` |
| Secondary data store | Redis (`REDIS_URL`) for cache/session-adjacent runtime data, **not** relational records |

## Connection model

The backend connects to PostgreSQL through a single shared SQLx connection pool.

### Runtime flow

1. `Config::from_env()` reads `DATABASE_URL` from the environment.
2. `create_pool()` builds a PostgreSQL connection pool with `PgPoolOptions`.
3. The server starts only after the pool has connected successfully.
4. After connecting, the backend runs every migration in `./migrations` before serving requests.

### Application connection details

- The database URL is **required**.
- The app creates a pool with a maximum of **10 concurrent database connections**.
- Migrations are applied automatically on startup.
- If connection or migration setup fails, the service exits instead of running with a partial schema.

### Local Docker setup

In Docker Compose, PostgreSQL is started as its own service and the backend receives this connection string shape:

```text
postgresql://<user>:<password>@127.0.0.1:5432/<database>
```

Default local values in `docker-compose.yml` are:

- Host: `127.0.0.1`
- Port: `5432`
- User: `teletable`
- Database: `teletable_db`

### Test setup

Integration tests connect using:

- `TEST_DATABASE_URL`, or
- `DATABASE_URL` as a fallback.

The test helper uses a smaller pool size of **5** connections and also runs migrations before tests execute.


## Schema overview

The relational schema currently has three core tables:

- `users` stores account identity, credentials, and role.
- `diary_entries` stores work-log entries owned by a user.
- `sessions` stores login session history and client metadata for a user.

There are also two convenience views:

- `user_last_sign_on` derives each user's last sign-on time from `sessions`.
- `current_sessions` derives the newest session per user from `sessions`.

## ER model

```mermaid
erDiagram
    USERS ||--o{ DIARY_ENTRIES : owns
    USERS ||--o{ SESSIONS : creates

    USERS {
        UUID id PK
        VARCHAR name
        VARCHAR email UK
        VARCHAR password_hash
        VARCHAR role
        TIMESTAMPTZ created_at
    }

    DIARY_ENTRIES {
        UUID id PK
        UUID owner FK
        INTEGER working_minutes
        TEXT text
        TIMESTAMPTZ created_at
        TIMESTAMPTZ updated_at
    }

    SESSIONS {
        UUID id PK
        UUID user_id FK
        TEXT ip_address
        JSONB fingerprint_data
        TEXT user_agent
        TIMESTAMPTZ created_at
    }
```


## Table reference

### `users`

Stores application users and their authentication metadata.

| Column | Type | Null | Default | Purpose |
| ------ | ---- | ---- | ------- | ------- |
| `id` | `UUID` | No | `gen_random_uuid()` | Primary key for the user |
| `name` | `VARCHAR(255)` | No | None | Display name |
| `email` | `VARCHAR(255)` | No | None | Unique login identifier |
| `password_hash` | `VARCHAR(255)` | No | None | Bcrypt-hashed password |
| `role` | `VARCHAR(50)` | No | `'Viewer'` | Authorization role |
| `created_at` | `TIMESTAMP WITH TIME ZONE` | Yes at insert time | `NOW()` | Account creation timestamp |

#### Behavior notes

- `email` is unique, so duplicate registrations are rejected at both application and database level.
- `role` originally defaulted to `'user'`, but a later migration changed the default to `'Viewer'` and migrated existing `'user'` rows.
- The role domain is enforced by application logic (`Admin`, `Operator`, `Viewer`). A database `CHECK` constraint was considered in a migration comment but is **not currently enabled**.
- Last sign-on is derived from `sessions` through the `user_last_sign_on` view.

#### Indexes

- `idx_users_email` on `email`

This supports fast login and duplicate-email checks.

### `diary_entries`

Stores time-tracked diary/work-log entries.

| Column | Type | Null | Default | Purpose |
| ------ | ---- | ---- | ------- | ------- |
| `id` | `UUID` | No | `gen_random_uuid()` | Primary key for the entry |
| `owner` | `UUID` | No | None | References `users.id` |
| `working_minutes` | `INTEGER` | No | None | Minutes worked for the entry |
| `text` | `TEXT` | No | None | Free-form diary content |
| `created_at` | `TIMESTAMP WITH TIME ZONE` | Yes at insert time | `NOW()` | Creation timestamp |
| `updated_at` | `TIMESTAMP WITH TIME ZONE` | Yes at insert time | `NOW()` | Last update timestamp |

#### Behavior notes

- `owner` is a foreign key to `users(id)`.
- The relation uses `ON DELETE CASCADE`, so deleting a user automatically deletes all of that user's diary entries.
- The backend uses ownership checks in queries, so users can only update or delete their own entries.
- `updated_at` is set by a PostgreSQL trigger on every `UPDATE`, so it cannot silently drift.
- `text` has a database-level `CHECK` constraint: maximum 5000 characters.

#### Indexes

- `idx_diary_entries_owner` on `owner`
- `idx_diary_entries_created_at` on `created_at`

These support fast owner-based lookups and reverse-chronological listing.

### `sessions`

Stores login session history and device/client metadata.

| Column | Type | Null | Default | Purpose |
| ------ | ---- | ---- | ------- | ------- |
| `id` | `UUID` | No | `gen_random_uuid()` | Primary key for the session row |
| `user_id` | `UUID` | No | None | References `users.id` |
| `ip_address` | `TEXT` | No | None | Captured client IP |
| `fingerprint_data` | `JSONB` | No | `'{}'::jsonb` | Structured client/device fingerprint payload |
| `user_agent` | `TEXT` | Yes | None | Raw HTTP user agent string |
| `created_at` | `TIMESTAMP WITH TIME ZONE` | No | `NOW()` | Session creation timestamp |

#### Behavior notes

- `user_id` is a foreign key to `users(id)`.
- The relation uses `ON DELETE CASCADE`, so deleting a user also deletes that user's session history.
- On login and registration, the backend inserts a new session row.
- The "current" session is derived by the `current_sessions` view (`DISTINCT ON (user_id)` ordered by `created_at DESC`).

#### Indexes

- `idx_sessions_user_id` on `user_id`
- `idx_sessions_created_at` on `created_at DESC`
- `idx_sessions_user_id_created_at` on `(user_id, created_at DESC)`

These support user session history queries and recent-session ordering.

## Views

### `user_last_sign_on`

```sql
SELECT user_id, MAX(created_at) AS last_sign_on
FROM sessions
GROUP BY user_id;
```

Provides a per-user derived sign-on timestamp without duplicating data in `users`.

### `current_sessions`

```sql
SELECT DISTINCT ON (user_id) *
FROM sessions
ORDER BY user_id, created_at DESC;
```

Provides one latest session row per user.


## Special data types

### `UUID`

`UUID` is used as the primary key type for all core tables.

- It gives globally unique identifiers without relying on sequential integers.
- It is a good fit for distributed systems and API-facing identifiers.
- In this backend, IDs can be generated in Rust with `Uuid::new_v4()` **or** by PostgreSQL defaults (`gen_random_uuid()`) on core table primary keys.

### `TIMESTAMP WITH TIME ZONE`

PostgreSQL stores these values as timezone-aware timestamps, often referred to as `timestamptz`.

- Used for `created_at` and `updated_at` (and derived `last_sign_on` in `user_last_sign_on`).
- `NOW()` sets the value at insert/update time in the database.
- In Rust, these fields map to `chrono::DateTime<Utc>`.
- This avoids ambiguity compared with local-time timestamps.

### `JSONB`

`JSONB` stores structured JSON in a binary PostgreSQL format.

- Used by `sessions.fingerprint_data`.
- Lets the backend store variable client/device metadata without creating many nullable columns.
- Supports JSON querying and indexing later if the project ever needs more advanced reporting.
- Defaults to an empty JSON object: `'{}'::jsonb`.
