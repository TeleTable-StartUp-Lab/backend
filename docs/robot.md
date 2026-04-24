# Robot API

This document describes the current robot-related HTTP and WebSocket API implemented by the backend.

## Quick reference

| Method   | Path                           | Auth         | Purpose |
| -------- | ------------------------------ | ------------ | ------- |
| GET (WS) | `/ws/robot/control`            | Public       | Stream backend robot commands to robot client(s) |
| POST     | `/table/register`              | None         | Register robot URL with backend (robot -> backend) |
| POST     | `/table/state`                 | `X-Api-Key`  | Robot telemetry update (robot -> backend) |
| POST     | `/table/event`                 | `X-Api-Key`  | Robot notification event (robot -> backend) |
| GET      | `/nodes`                       | JWT (Bearer) | Get static navigation nodes from backend app state |
| GET      | `/routes`                      | JWT (Bearer) | Get current route queue |
| POST     | `/routes`                      | JWT (Admin)  | Add route to queue |
| DELETE   | `/routes/{id}`                 | JWT (Admin)  | Remove route from queue |
| POST     | `/routes/optimize`             | JWT (Admin)  | Trigger route optimization |
| POST     | `/routes/select`               | JWT (Bearer) | Queue route selection (blocked while manual lock active) |
| POST     | `/drive/lock`                  | JWT (Bearer) | Acquire manual drive lock (30s expiry set on acquire) |
| DELETE   | `/drive/lock`                  | JWT (Bearer) | Release manual drive lock (only holder can release) |
| GET      | `/robot/check`                 | JWT (Bearer) | Probe registered robot via `GET {robot_url}/health` |
| GET      | `/robot/debug`                 | JWT (Admin)  | Get admin debug snapshot for dashboard polling |
| GET      | `/robot/notifications`         | JWT (Viewer+) | Get persisted robot notification history |
| GET (WS) | `/ws/drive/manual?token=<jwt>` | JWT in query | Manual control command socket (input only) |
| GET (WS) | `/ws/robot/events?token=<jwt>` | JWT in query | Status + notification event socket (output only) |

## Key architectural note

The old polling status endpoint was removed.

- There is no `GET /status` endpoint.
- Status is pushed as WebSocket events on `/ws/robot/events`.
- Notification events are also pushed on `/ws/robot/events`.
- Manual control on `/ws/drive/manual` is command input only.
- Admin debug snapshots are fetched over HTTP from `GET /robot/debug`; the dashboard polls while the debug panel is open.

## In-memory robot state

The backend maintains `SharedRobotState` with:

- `current_state`: last telemetry (`RobotState`)
- `last_state_update`: time of latest `/table/state`
- `robot_url`: discovered robot base URL (from `/table/register`)
- `static_nodes`: predefined navigation nodes stored in app state
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
  "targetNode": "Kitchen",
  "gyroscope": {
    "xDps": 0.3,
    "yDps": -0.1,
    "zDps": 1.2
  },
  "lastReadUuid": "04A1B2C3D4",
  "lux": 124.5,
  "infrared": {
    "front": false,
    "left": true,
    "right": false
  },
  "voltageV": 12.4,
  "currentA": 1.6,
  "powerW": 19.8
}
```

Fields:

- `systemHealth` (`string`, required): overall robot health string such as `OK`.
- `batteryLevel` (`number`, required): battery percentage from `0` to `100`.
- `driveMode` (`string`, required): backend-visible robot mode such as `IDLE`, `MANUAL`, or `AUTO`.
- `cargoStatus` (`string`, required): current cargo/load state string.
- `currentPosition` (`string`, required): current robot position or location label.
- `lastNode` (`string`, optional): previously visited node for the active or last route.
- `targetNode` (`string`, optional): destination node for the active or last route.
- `gyroscope` (`object`, optional): angular velocity readings in degrees per second.
- `gyroscope.xDps` (`number`, optional): X-axis angular velocity.
- `gyroscope.yDps` (`number`, optional): Y-axis angular velocity.
- `gyroscope.zDps` (`number`, optional): Z-axis angular velocity.
- `lastReadUuid` (`string`, optional): last RFID UUID observed by the firmware.
- `lux` (`number`, optional): ambient light reading in lux.
- `infrared` (`object`, optional): infrared obstacle sensor readings.
- `infrared.front` (`boolean`, optional): front obstacle state.
- `infrared.left` (`boolean`, optional): left obstacle state.
- `infrared.right` (`boolean`, optional): right obstacle state.
- `voltageV` (`number`, optional): measured battery voltage in volts.
- `currentA` (`number`, optional): measured battery current in amps.
- `powerW` (`number`, optional): measured battery power in watts.

Notes:

- JSON keys are camelCase.
- Any optional field may be omitted entirely.
- Optional nested objects may also be sent partially, for example only `gyroscope.zDps`.

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

- accepts the extended `RobotState` telemetry payload documented above
- stores the last received telemetry payload in `current_state`
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

- returns the static node list stored in backend app state
- does not query Redis or the robot

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
- appends the route to the in-memory queue
- then calls queue processing, which may dispatch it immediately if the robot is connected, idle, unlocked, and no other route is active
- does not directly send a navigation command from this handler

Success:

```json
{ "status": "success", "message": "Route queued" }
```

Lock conflict response:

- HTTP status remains `200 OK`
- body is:

```json
{ "status": "error", "message": "Robot is manually locked" }
```

## `GET /routes`

Behavior:

- returns a JSON array of routes
- if an `active_route` exists, it is returned as the first element
- queued routes follow in FIFO order

Example:

```json
[
  {
    "id": "uuid",
    "start": "Home",
    "destination": "Kitchen",
    "added_at": "2026-03-26T12:34:56Z",
    "added_by": "Admin User"
  }
]
```

## `POST /drive/lock` and `DELETE /drive/lock`

Behavior:

- lock expires after 30 seconds
- Operator/Admin only
- broadcasts `status_update` after successful acquire/release
- non-admin users cannot acquire the lock while an automated route is active
- if another non-expired lock is held:
  - non-admin acquire returns HTTP `200` with `{ "status": "error", "message": "Lock held by <name>" }`
  - admin acquire replaces the existing lock holder
- `DELETE /drive/lock` only succeeds for the current lock holder, even for admins

Successful acquire example:

```json
{ "status": "success", "message": "Lock acquired" }
```

Admin acquire while an automated route is active:

```json
{ "status": "success", "message": "Admin lock acquired while automated route is active" }
```

Failure examples use HTTP `200 OK` with JSON error payloads, for example:

```json
{ "status": "error", "message": "Cannot acquire lock while automated route is active" }
```

```json
{ "status": "error", "message": "You do not hold the lock" }
```

## `GET /robot/check`

Behavior:

- checks staleness first
- if connected, probes `GET {robot_url}/health`

## `GET /robot/debug`

Auth:

- Admin only

Behavior:

- builds an admin-only debug snapshot from backend in-memory state
- enriches sensor fields with robot `GET {robot_url}/status` when reachable
- uses the same static node list returned by `GET /nodes`
- intended for HTTP polling by the dashboard while the debug modal is open

Returns:

```json
{
  "telemetry": {
    "systemHealth": "OK",
    "batteryLevel": 82,
    "driveMode": "IDLE",
    "cargoStatus": "EMPTY",
    "position": "Home",
    "lastRoute": { "start_node": "Home", "end_node": "Kitchen" },
    "robotConnected": true
  },
  "lock": {
    "holderName": null,
    "active": false,
    "expiresAt": null
  },
  "routing": {
    "activeRoute": null,
    "queue": [],
    "queueLength": 0,
    "nodes": ["Home", "Kitchen", "Office"]
  },
  "connection": {
    "robotUrl": "http://robot.local:8000",
    "lastStateUpdate": "2026-03-26T13:05:00Z",
    "robotStatusReachable": true
  },
  "sensors": {
    "light": { "lux": 124.5, "valid": true, "source": "robot_status_http" },
    "infrared": { "front": false, "left": true, "right": true, "source": "robot_status_http" },
    "power": { "voltageV": 12.4, "currentA": 1.6, "powerW": 19.8, "source": "robot_status_http" },
    "gyroscope": { "xDps": null, "yDps": null, "zDps": null, "source": "unavailable" },
    "rfid": { "lastReadUuid": null, "source": "unavailable" }
  }
}
```

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
- this socket is output-only from backend to robot clients

## `GET /ws/drive/manual?token=<jwt>`

Purpose:

- user manual control input socket

Auth:

- JWT token in query
- the token is decoded directly and does not pass through the HTTP auth middleware role-refresh path

Behavior:

- processes incoming command frames only
- does not stream status/notifications
- Viewer connections are accepted, but all incoming commands are ignored
- Operator commands require a valid, unexpired lock held by that same operator
- Operator can only send manual drive commands (`DRIVE_COMMAND`)
- Operator cannot send `NAVIGATE`, `CANCEL`, `LED`, `AUDIO_BEEP`, or `AUDIO_VOLUME`
- Admin can send all commands
- Admin `NAVIGATE`:
  - revokes another user's lock if needed
  - cancels the current active automated route if one exists
  - re-queues that automated route at the front of the queue
  - tracks the admin navigation as the new `active_route`

## `GET /ws/robot/events?token=<jwt>`

Purpose:

- server push socket for robot status and notifications

Auth:

- JWT token in query
- Viewer or higher
- the token is decoded directly and does not pass through the HTTP auth middleware role-refresh path

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
