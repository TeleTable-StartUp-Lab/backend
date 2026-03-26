# Robot API

This document describes the current robot-related HTTP and WebSocket API implemented by the backend.

## Quick reference

| Method   | Path                           | Auth         | Purpose |
| -------- | ------------------------------ | ------------ | ------- |
| GET (WS) | `/ws/robot/control`            | Public       | Stream backend robot commands to robot client(s) |
| POST     | `/table/register`              | None         | Register robot URL with backend (robot -> backend) |
| POST     | `/table/state`                 | `X-Api-Key`  | Robot telemetry update (robot -> backend) |
| POST     | `/table/event`                 | `X-Api-Key`  | Robot notification event (robot -> backend) |
| GET      | `/nodes`                       | JWT (Bearer) | Get robot nodes (cached; fetched from registered robot HTTP server) |
| GET      | `/routes`                      | JWT (Bearer) | Get current route queue |
| POST     | `/routes`                      | JWT (Admin)  | Add route to queue |
| DELETE   | `/routes/{id}`                 | JWT (Admin)  | Remove route from queue |
| POST     | `/routes/optimize`             | JWT (Admin)  | Trigger route optimization |
| POST     | `/routes/select`               | JWT (Bearer) | Queue route selection (blocked while manual lock active) |
| POST     | `/drive/lock`                  | JWT (Bearer) | Acquire manual drive lock (30s expiry set on acquire) |
| DELETE   | `/drive/lock`                  | JWT (Bearer) | Release manual drive lock (only holder can release) |
| GET      | `/robot/check`                 | JWT (Bearer) | Probe registered robot via `GET {robot_url}/health` |
| GET      | `/robot/notifications`         | JWT (Viewer+) | Get persisted robot notification history |
| GET (WS) | `/ws/drive/manual?token=<jwt>` | JWT in query | Manual control command socket (input only) |
| GET (WS) | `/ws/robot/events?token=<jwt>` | JWT in query | Status + notification event socket (output only) |

## Key architectural note

The old polling status endpoint was removed.

- There is no `GET /status` endpoint.
- Status is pushed as WebSocket events on `/ws/robot/events`.
- Notification events are also pushed on `/ws/robot/events`.
- Manual control on `/ws/drive/manual` is command input only.

## In-memory robot state

The backend maintains `SharedRobotState` with:

- `current_state`: last telemetry (`RobotState`)
- `last_state_update`: time of latest `/table/state`
- `robot_url`: discovered robot base URL (from `/table/register`)
- `cached_nodes`: cached robot nodes
- `manual_lock`: lock holder and expiry
- `command_sender`: broadcast channel for `RobotCommand` (used by `/ws/robot/control`)
- `status_sender`: broadcast channel for `status_update` events
- `notification_sender`: broadcast channel for `robot_notification` events
- `queue`: pending routes
- `active_route`: currently executing queued route

## Robot connection staleness

Robot is considered connected when `last_state_update` is within 30 seconds (`ROBOT_STALE_TIMEOUT_SECS`).

A cleanup task runs every 5 seconds and:

- clears expired manual locks
- clears stale `active_route`
- clears stale `robot_url`

## Data types

### `RobotState` (robot telemetry)

Expected camelCase JSON:

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

`lastNode` and `targetNode` are optional.

### `RobotEvent` (robot -> backend)

Expected JSON:

```json
{
  "priority": "INFO",
  "message": "Route started: Home -> Kitchen"
}
```

`priority` must be one of:

- `INFO`
- `WARN`
- `ERROR`

### Persisted notification (`robot_notifications`)

```json
{
  "id": "uuid",
  "priority": "WARN",
  "message": "Low battery: 18%",
  "receivedAt": "2026-03-26T12:34:56Z"
}
```

### `RobotCommand` (over WebSocket)

Tagged JSON with `command`:

- `NAVIGATE`
- `CANCEL`
- `DRIVE_COMMAND`
- `LED`
- `AUDIO_BEEP`
- `AUDIO_VOLUME`

## Robot-to-backend endpoints

## `POST /table/state`

Auth:

- `X-Api-Key` required and must match `ROBOT_API_KEY`

Behavior:

- updates `current_state`
- updates `last_state_update`
- clears `active_route` when robot returns to `driveMode: "IDLE"`
- triggers queue processing
- broadcasts `status_update` on `/ws/robot/events`

Response:

```json
{ "status": "success" }
```

Error:

```json
{ "status": "error", "message": "Invalid API Key" }
```

## `POST /table/event`

Auth:

- `X-Api-Key` required and must match `ROBOT_API_KEY`

Behavior:

- validates non-empty `message`
- persists notification in `robot_notifications`
- broadcasts `robot_notification` on `/ws/robot/events`

Success response:

```json
{
  "status": "success",
  "notification": {
    "id": "uuid",
    "priority": "INFO",
    "message": "Route started",
    "receivedAt": "2026-03-26T12:34:56Z"
  }
}
```

Errors:

- `401` invalid API key
- `400` empty message
- `500` DB insert failure

## `POST /table/register`

Behavior:

- registers robot URL from source IP + payload port
- prefers `X-Real-IP`, then first `X-Forwarded-For`, then socket IP
- broadcasts `status_update` on `/ws/robot/events`

Response:

- `200 OK`

## User JWT-protected robot endpoints

## `GET /nodes`

Behavior:

- checks Redis cache first
- then in-memory cache
- then robot `GET {robot_url}/nodes`

Returns:

```json
{ "nodes": ["Home", "Kitchen"] }
```

## `POST /routes/select`

Request:

```json
{ "start": "Home", "destination": "Kitchen" }
```

Behavior:

- requires Operator or Admin
- blocked by active manual lock
- route is queued, not directly executed

Success:

```json
{ "status": "success", "message": "Route queued" }
```

## `POST /drive/lock` and `DELETE /drive/lock`

Behavior:

- lock expires after 30 seconds
- Operator/Admin only
- broadcasts `status_update` after successful acquire/release

## `GET /robot/check`

Behavior:

- checks staleness first
- if connected, probes `GET {robot_url}/health`

## `GET /robot/notifications`

Auth:

- Viewer or higher

Query params:

- `limit` (default 100, min 1, max 500)
- `offset` (default 0)

Response:

```json
[
  {
    "id": "uuid",
    "priority": "ERROR",
    "message": "Robot emergency stop triggered",
    "receivedAt": "2026-03-26T13:00:00Z"
  }
]
```

## WebSockets

## `GET /ws/robot/control`

Purpose:

- backend -> robot command stream

Auth:

- public

Behavior:

- sends `RobotCommand` frames from `command_sender`
- robot client should connect here to receive commands

## `GET /ws/drive/manual?token=<jwt>`

Purpose:

- user manual control input socket

Auth:

- JWT token in query

Behavior:

- processes incoming command frames only
- does not stream status/notifications
- Viewer cannot send commands
- Operator restrictions apply (including lock checks)
- Admin preemption for `NAVIGATE` applies

## `GET /ws/robot/events?token=<jwt>`

Purpose:

- server push socket for robot status and notifications

Auth:

- JWT token in query
- Viewer or higher

Behavior:

- sends one initial `status_update` on connect
- streams subsequent:
  - `status_update`
  - `robot_notification`

`status_update` payload (camelCase keys):

```json
{
  "event": "status_update",
  "data": {
    "systemHealth": "OK",
    "batteryLevel": 82,
    "driveMode": "IDLE",
    "cargoStatus": "EMPTY",
    "position": "Home",
    "lastRoute": { "start_node": "Home", "end_node": "Kitchen" },
    "manualLockHolderName": null,
    "robotConnected": true,
    "nodes": ["Home", "Kitchen", "Office"]
  }
}
```

`robot_notification` payload:

```json
{
  "event": "robot_notification",
  "data": {
    "id": "uuid",
    "priority": "WARN",
    "message": "Low battery: 18%",
    "receivedAt": "2026-03-26T13:05:00Z"
  }
}
```

## Robot simulator contract

Robot simulator should:

- push telemetry with `POST {backend}/table/state` + `X-Api-Key`
- push notification events with `POST {backend}/table/event` + `X-Api-Key`
- receive commands from `ws://{backend}/ws/robot/control`

Clients (frontend/mobile) should subscribe to:

- `ws://{backend}/ws/robot/events?token=<jwt>` for status and notifications
