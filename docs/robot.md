# Robot API

This document describes the **robot-related HTTP + WebSocket API** implemented in the backend, plus the coupling/contract with the robot’s HTTP server (including the Python simulator in `firmware/robot_simulator.py`).

## Quick reference

| Method   | Path                           | Auth         | Purpose                                                             |
| -------- | ------------------------------ | ------------ | ------------------------------------------------------------------- |
| GET      | `/status`                      | Public       | Get current robot status (derived from cached telemetry + lock)     |
| GET (WS) | `/ws/robot/control`            | Public       | Stream backend robot commands to robot client(s)                    |
| POST     | `/table/register`              | None         | Register robot URL with backend (robot → backend)                   |
| POST     | `/table/state`                 | `X-Api-Key`  | Robot telemetry update (robot → backend)                            |
| POST     | `/table/event`                 | `X-Api-Key`  | Robot event reporting (robot → backend)                             |
| GET      | `/nodes`                       | JWT (Bearer) | Get robot nodes (cached; fetched from registered robot HTTP server) |
| GET      | `/routes`                      | JWT (Any)    | Get current route queue                                             |
| POST     | `/routes`                      | JWT (Admin)  | Add route to queue                                                  |
| DELETE   | `/routes/:id`                  | JWT (Admin)  | Remove route from queue                                             |
| POST     | `/routes/optimize`             | JWT (Admin)  | Trigger route optimization                                          |
| POST     | `/routes/select`               | JWT (Bearer) | Send `NAVIGATE` command (blocked while manual lock active)          |
| POST     | `/drive/lock`                  | JWT (Bearer) | Acquire manual drive lock (30s expiry set on acquire)               |
| DELETE   | `/drive/lock`                  | JWT (Bearer) | Release manual drive lock (only holder can release)                 |
| GET      | `/robot/check`                 | JWT (Bearer) | Probe registered robot via `GET {robot_url}/health`                 |
| GET (WS) | `/ws/drive/manual?token=<jwt>` | JWT in query | Send manual control commands; backend enforces lock-holder identity |

## Concepts and state

The backend maintains in-memory robot state in `SharedRobotState`:

- `current_state`: last reported robot telemetry (`RobotState`)
- `last_state_update`: timestamp of the most recent `/table/state` call (used for staleness detection)
- `robot_url`: registered robot base URL (via `/table/register`)
- `cached_nodes`: cached result of robot `/nodes`
- `manual_lock`: who holds manual drive lock and when it expires
- `command_sender`: broadcast channel for `RobotCommand` messages
- `queue`: Sequence of pending `QueuedRoute`s
- `active_route`: Currently executing route from queue

### Robot Connection Staleness

The backend tracks whether the robot is "connected" by comparing `last_state_update` against a **30-second timeout** (`ROBOT_STALE_TIMEOUT_SECS`). If the robot has not sent a `/table/state` update within this window, it is considered disconnected.

### Background Cleanup Task

A background task runs every **5 seconds** (`CLEANUP_INTERVAL_SECS`) and performs:

1. **Expired lock cleanup:** Clears any manual lock whose `expires_at` is in the past.
2. **Stale robot cleanup:** If the robot is stale (no updates in 30s):
   - Clears `active_route` so the route queue is not permanently stuck.
   - Clears `robot_url` so `/status` and `/robot/check` correctly reflect the disconnection.

### Queue & Preemption Logic

**Queue Processing:**
The backend processes the queue sequentially. A `NAVIGATE` command is sent to the robot only when all of the following conditions are met:

1. **No active (non-expired) manual lock** — expired locks are ignored.
2. **Robot is connected** — `last_state_update` is within the 30-second staleness threshold.
3. **Robot is IDLE** — `driveMode` equals `"IDLE"`.
4. **No active route** — `active_route` is `None`.
5. **Queue is non-empty.**

**Admin Preemption:**
If an **Admin** sends a navigation command via WebSocket while a queued route is active:

1. **Lock Revocation:** If an Operator holds the lock, it is forcibly revoked.
2. **Queue Re-ordering:** The currently active route is cancelled and moved to the **front** of the queue.
3. **Route Restart:** When resumed, the preempted route starts from its beginning.
4. **Immediate Execution:** The Admin's command is executed immediately.

### WS Command Filtering

The `/ws/drive/manual` endpoint enforces an allow-list: `NAVIGATE`, `DRIVE_COMMAND`, `SET_MODE`, `CANCEL`. Unauthorized commands are rejected.

### Data types

#### `RobotState` (robot telemetry)

Backend expects camelCase JSON:

```json
{
  "systemHealth": "OK",
  "batteryLevel": 85,
  "driveMode": "IDLE",
  "cargoStatus": "EMPTY",
  "currentPosition": "Home",
  "lastNode": "Home",
  "targetNode": "Kitchen"
}
```

Notes:

- `lastNode` and `targetNode` are optional (`null` is allowed).

#### `RobotEvent`

Backend expects camelCase JSON and an RFC3339 timestamp parseable by `chrono`:

```json
{
  "event": "DESTINATION_REACHED",
  "timestamp": "2026-01-16T10:00:00Z"
}
```

#### `RobotCommand` (sent over WebSocket)

Backend serializes commands as JSON text frames with a tagged enum (`command`):

- Navigate:

```json
{ "command": "NAVIGATE", "start": "Home", "destination": "Kitchen" }
```

- Cancel:

```json
{ "command": "CANCEL" }
```

- Set mode:

```json
{ "command": "SET_MODE", "mode": "MANUAL" }
```

- Drive command (note snake_case field names):

```json
{ "command": "DRIVE_COMMAND", "linear_velocity": 0.5, "angular_velocity": 0.2 }
```

---

## Public endpoints

### `GET /status`

Return robot status for UI/clients.

#### Auth

- No authentication required.

#### Behavior

- If the robot has never reported telemetry, returns default values (`UNKNOWN`, `0`, etc.).
- Computes `lastRoute` only when both `lastNode` and `targetNode` exist in the current telemetry.
- Includes `manualLockHolderName` **only if the lock has not expired**. Expired locks are treated as if no lock exists.
- Includes `robotConnected` indicating whether the robot has sent a state update within the last 30 seconds.

#### Response (`200 OK`)

```json
{
  "systemHealth": "OK|UNKNOWN",
  "batteryLevel": 85,
  "driveMode": "IDLE|UNKNOWN",
  "cargoStatus": "EMPTY|UNKNOWN",
  "lastRoute": { "start_node": "Home", "end_node": "Kitchen" },
  "position": "Home|UNKNOWN",
  "manualLockHolderName": "Alice",
  "robotConnected": true
}
```

`lastRoute` and `manualLockHolderName` can be `null`.

### `GET /ws/robot/control`

WebSocket that **streams robot commands** from backend to connected clients.

#### Auth

- No authentication required.

#### Behavior

- The backend **only sends** messages; it does not read incoming frames.
- For each broadcasted `RobotCommand`, the backend sends a text frame containing JSON.

#### Error/close behavior

- When sending fails (e.g., client disconnected), the backend stops the loop and the socket closes.

---

## Robot-to-backend (API key protected)

These endpoints are called by the robot (or robot simulator).

### Authentication

Requests must include:

- `X-Api-Key: <robot api key>`

The expected value is `ROBOT_API_KEY` (defaults to `secret-robot-key`).

### `POST /table/state`

Update the backend’s cached telemetry.

#### Request

- Header: `X-Api-Key`
- Body: `RobotState` JSON (camelCase)
#### Behavior

- Stores the telemetry in `current_state`.
- Records `last_state_update` to the current UTC timestamp (used for staleness detection).
- If an `active_route` exists and the robot reports `driveMode: "IDLE"`, the active route is cleared (route completed).
- Triggers queue processing after updating state.
#### Responses

- `200 OK`:

```json
{ "status": "success" }
```

- `401 Unauthorized` if API key is missing/invalid:

```json
{ "status": "error", "message": "Invalid API Key" }
```

### `POST /table/event`

Report a robot event.

#### Request

- Header: `X-Api-Key`
- Body: `RobotEvent` JSON (camelCase)

#### Responses

- `200 OK`:

```json
{ "status": "success" }
```

- `401 Unauthorized` if API key is missing/invalid (same as `/table/state`).

---

## User-to-backend robot control (JWT protected)

These endpoints require:

- `Authorization: Bearer <jwt>`

### `GET /nodes`

Return the robot’s navigable nodes.

#### Behavior

- If nodes are cached in memory, returns cached nodes.
- Otherwise, if a robot URL is known, performs `GET {robot_url}/nodes` and expects:

```json
{ "nodes": ["Home", "Kitchen"] }
```

- On successful fetch and parse, caches nodes forever (until backend restart).

#### Responses

- `200 OK`:

```json
{ "nodes": ["Home", "Kitchen"] }
```

- `503 Service Unavailable` (robot unknown or fetch/parsing failed):

```json
{ "nodes": [] }
```

### `POST /routes/select`

Broadcast a `NAVIGATE` command.

#### Request

```json
{ "start": "Home", "destination": "Kitchen" }
```

#### Lock interaction

- If a manual lock exists and `expires_at > now`, navigation is blocked.

#### Responses (note: **HTTP status is always 200**)

- If locked:

```json
{ "status": "error", "message": "Robot is manually locked" }
```

- If accepted:

```json
{ "status": "success", "message": "Route selected" }
```

### `POST /drive/lock`

Acquire the manual-drive lock.

#### Behavior

- If another user holds a non-expired lock, acquisition is refused (unless requester is Admin, who can forcibly revoke).
- On success, lock expiry is set to `now + 30 seconds`.
- **Lock renewal:** The same user can re-acquire the lock to extend its expiry. The frontend automatically renews every 15 seconds to prevent silent expiry during active use.
- **Operators** cannot acquire the lock while an automated route is active. **Admins** can.

#### Responses (note: **HTTP status is always 200**)

- Success:

```json
{ "status": "success", "message": "Lock acquired" }
```

- Refused (held by someone else):

```json
{ "status": "error", "message": "Lock held by <name>" }
```

- Invalid `sub` in JWT (not a UUID):

```json
{ "status": "error", "message": "Invalid User ID" }
```

### `DELETE /drive/lock`

Release the manual-drive lock.

#### Responses (note: **HTTP status is always 200**)

- Success:

```json
{ "status": "success", "message": "Lock released" }
```

- Not lock holder:

```json
{ "status": "error", "message": "You do not hold the lock" }
```

### `GET /robot/check`

Check if the backend can reach the discovered robot.

#### Behavior

- First checks **staleness**: if the robot has not sent a `/table/state` update within the last 30 seconds, it is reported as disconnected without attempting an HTTP health check.
- If not stale and a robot URL is known, performs `GET {robot_url}/health`.
- Returns the HTTP status code returned by the robot in `robot_status`.

#### Responses (note: **HTTP status is always 200**)

- Success (reachable and not stale):

```json
{ "status": "success", "connected": true, "robot_status": 200, "url": "http://..." }
```

- Error (robot registered but stale):

```json
{
  "status": "error",
  "connected": false,
  "message": "Robot registered but no recent state updates (stale)",
  "url": "http://..."
}
```

- Error (request failed):

```json
{
  "status": "error",
  "connected": false,
  "message": "Failed to reach robot: ...",
  "url": "http://..."
}
```

- Error (no robot registered):

```json
{ "status": "error", "connected": false, "message": "No robot URL registered" }
```

---

## Manual control WebSocket (JWT token in query)

### `GET /ws/drive/manual?token=<jwt>`

WebSocket used to send manual control commands (as `RobotCommand` JSON text frames) into the backend.

#### Auth

- Requires `token` query parameter containing a valid JWT.
- If token is invalid/expired, the backend returns `401 Unauthorized` (no JSON body).

#### Lock enforcement

- For each incoming text frame, the backend checks whether the sender is the current lock holder **and** whether the lock has not expired.
- If the sender is not the holder, or the lock has expired, the command is silently ignored.
- If the command cannot be parsed as `RobotCommand`, it is silently ignored.

#### Operator restrictions

- Operators **cannot** send `NAVIGATE` or `CANCEL` commands via WebSocket; those are silently dropped.
- Operators must hold a valid (non-expired) lock to send any other command.

#### Admin behavior

- Admins can send any command without holding the lock.
- If an Admin sends a `NAVIGATE` command while an Operator holds the lock, the lock is forcibly revoked.
- Admin navigation preempts any active queued route (cancels it and moves it to the front of the queue).

---

## Firmware/Python simulator coupling and contracts

The backend expects the robot to implement (at minimum) these HTTP endpoints, and the robot expects the backend to implement the robot endpoints above.

### Python simulator HTTP API (robot side)

The Python simulator in `firmware/robot_simulator.py` starts a Flask server (default `0.0.0.0:8000`) with:

- `GET /health` → `200 OK`

```json
{ "status": "ok", "message": "Robot is online" }
```

- `GET /status` → `200 OK` returning the current in-process telemetry dict (same shape as `RobotState`)
- `GET /nodes` → `200 OK`

```json
{
  "nodes": [
    "Home",
    "Kitchen",
    "Living Room",
    "Office",
    "Bedroom",
    "Charging Station"
  ]
}
```

### Robot discovery (UDP)

- Backend listens on UDP `0.0.0.0:3001`.
- Robot should broadcast a JSON message of the form:

```json
{ "type": "announce", "port": 8000 }
```

- Backend records `robot_url` as `http://<sender_ip>:<port>`.

The Python simulator sends this broadcast every 10 seconds.

### Robot HTTP server contract (robot side)

The backend calls:

- `GET {robot_url}/health`
  - Used by `/robot/check`
  - Only the HTTP status code is used.

- `GET {robot_url}/nodes`
  - Used by `/nodes`
  - Must return JSON parseable as:

```json
{ "nodes": ["Home", "Kitchen"] }
```

### Robot pushing telemetry/events (robot → backend)

The Python simulator (and real robot) are expected to call:

- `POST {backend}/table/state` with `X-Api-Key` and `RobotState` JSON
- `POST {backend}/table/event` with `X-Api-Key` and `RobotEvent` JSON

### Robot receiving commands (backend → robot)

The Python simulator connects to:

- `ws://{backend}/ws/robot/control`

and expects each text frame to be a JSON object containing a `command` field (see `RobotCommand` above).
