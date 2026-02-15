# TeleTable Backend

Rust backend service for TeleTable.

It provides:

- User authentication and authorization (JWT)
- Diary entry CRUD backed by PostgreSQL
- Robot coordination (HTTP + WebSocket)
- Three-tier RBAC (Admin, Operator, Viewer)

## User Roles (RBAC)

The system implements the following roles:

- **Admin:** Full system access. Can overwrite active routes via WebSocket, forcibly revoke manual mode locks, and manage users.
- **Operator:** Can select routes, create diary entries, and acquire "manual mode" locks.
- **Viewer:** Default read-only access. Cannot create/update/delete diary entries or control the robot.

Role changes are enforced **immediately** — the auth middleware refreshes the user's role from the database on every request, so demoting a user takes effect without requiring them to log out.

## Key behaviors

- **Lock expiry:** Manual drive locks expire after 30 seconds. The frontend renews them automatically every 15 seconds. Expired locks are cleaned up by a background task and ignored by all endpoints.
- **Robot staleness detection:** If the robot has not sent a state update in 30 seconds, it is considered disconnected. A background task clears the stale `robot_url` and any stuck `active_route`.
- **Background cleanup:** A task runs every 5 seconds to clear expired locks and stale robot state, preventing stuck queues and phantom lock holders.

## Tech stack

- Rust (edition 2021)
- Axum (HTTP + WebSocket)
- SQLx + PostgreSQL
- Redis connection (wired up in app state; can be used for caching/session features)
- JWT via `jsonwebtoken`, password hashing via `bcrypt`

## Running locally (Docker)

From this directory:

```bash
docker compose up --build
```

This starts:

- Postgres
- Redis
- Backend on `http://localhost:3003`

## Configuration

The backend reads configuration from environment variables:

- `DATABASE_URL` (required)
- `REDIS_URL` (required)
- `JWT_SECRET` (required)
- `JWT_EXPIRY_HOURS` (optional, default `24`)
- `SERVER_ADDRESS` (optional, default `0.0.0.0:3003`)
- `ROBOT_API_KEY` (optional, default `secret-robot-key`)

## API documentation

Detailed, implementation-accurate API docs are split by module:

- [docs/auth.md](docs/auth.md)
- [docs/diary.md](docs/diary.md)
- [docs/robot.md](docs/robot.md)

The root endpoint `GET /` returns a plain-text banner string and is used as the Docker health check.
