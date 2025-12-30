# Routes

## Login

- User:

```json
{
  "id": 0,
  "name": "user",
  "email": "email@example.com",
  "role": "admin/user"
}
```

- `POST /register`
  - Accept email/username + password.
  - Hash password and store in DB.

- `POST /login`
  - Verify credentials.
  - Generate JWT (usually with a short expiry, e.g., 15–60 min).
  - Return JWT to client in JSON ({ "token": "<jwt>" }).

- `GET /me`
  - Verify credentials.
  - Decode claims & fetch information about the user
  - return information
- `GET /user?id=x` (admin only)
  - die User bekommen und bearbeiten koennen entweder all oder einen mit id

- `POST /user` (admin only)
  - den spezifischen user bearbeiten

## Tagebuch

immer **Authorization: Bearer <jwt>**

- Diary:

```json
{
  "owner": "<user id>",
  "id": 0,
  "working_minutes": "<how long u worked today in minutes>",
  "text": "<was du heute genau getan hast>"
}
```

- `POST /diary`
  - Erstellen oder aendern eines Tagebuch eintrags

- `GET /diary`
  - Alle eintraege

- `GET /diary?id=x`
  - spezifischer tagebuch eintrag

- `DELETE /diary`
  - loeschen eines tagebuch eintrags

## Car Routes

- `GET /status`

  ```json
  {
    "systemHealth": "OK", //could be "OK", "WARNING", "ERROR", "OFFLINE"
    "batteryLevel": 64,
    "driveMode": "MANUAL/AUTO/IDLE",
    "cargoStatus": "LOADING", // Status des transportierten Gegenstands: "LOADING", "IN_TRANSIT", "DELIVERY_CONFIRMED", "EMPTY"
    "lastRoute": { "startNode": "Mensa", "endNode": "Zimmer 101" },
    "position": "Zimmer 101....", // kann auch "UNKNOWN" sein falls manual drive aktiv ist
    // TODO: Positioning muss noch genauer spizifiziert werden/
    "manualLockHolderName": "<name des lock hodlers>"
  }
  ```

- `GET /nodes`
  - Liste der waehlbaren routen

  ```json
  {
    "nodes": ["Mensa", "Zimmer 101", "Zimmer 102", "..."]
  }
  ```

- `POST /routes/select`
  - waehle eine route aus
  - nachdem der Tisch die Route bekommen hat begibt er sich zum start, idelt dort und geht dann sobald der start knopf auf dem roboter gedrueckt wurde zur destiniation.
  - Falls der Roboter in der Zwischenzeit auf Manual geschaltet wurde geht er immer zum letzten ziel hin

  ```json
  {
    "start": "Mensa",
    "destination": "Zimmer 101"
  }
  ```

- `POST/DELETE /drive/lock`
  - drive lock anfordern
  - die Nuter id bei post aud dem auth header nehmen und den user als lock holder einspeichern
  - nur der lock holder kann das lock loeschen
  - Das Backend verwaltet den Lock-Zustand (manualLockHolderId), um zu gewährleisten, dass nur ein Nutzer die manuelle Steuerung innehat.
  - Zuweisung: Das Backend prüft, ob der Lock frei ist (manualLockHolderId ist null).

## Websocket

`ws://backend-server/ws/drive/manual`

- wird geoffnet sobal man das drive lock angefordert hat, andere fahrer koennen in der Zeit nicht das lock holen oder auf automatic umstellen
- Verbindungsaufbau: Der Client öffnet den WebSocket nachdem er den Lock erfolgreich über HTTP angefordert hat.
- Autorisierung: Die WebSocket-Verbindung wird nur zugelassen, wenn die UserID des verbindenden Clients mit der aktuellen manualLockHolderId übereinstimmt.
- Kommandobeschränkung: Nur der Client, der den Lock hält, darf Steuerbefehle über diesen Socket senden.

### Sicherheit

- Timer starten: Beim Zuweisen des Locks startet das Backend einen Timeout-Timer (z.B. 5 Sekunden).
- Automatische Freigabe bei timerauslauf, wenn die website geoffnet ist kann der client ein Awake ping schicken.

## Robot Connectivity (Backend <-> Roboter)

Diese Routen dienen dazu, dass der Roboter seinen Zustand meldet und Befehle vom Backend empfängt.

### 1. Telemetrie & Status (Robot -> Backend)

Der Roboter sendet regelmäßig (z.B. alle 1-5 Sekunden) seinen internen Zustand an das Backend, damit die `GET /status` Route für die Nutzer aktuelle Daten liefert.

- `POST /table/state`
- **Auth**: API-Key (fest im Roboter hinterlegt)
- **Payload**:

```json
{
  "systemHealth": "OK",
  "batteryLevel": 85,
  "driveMode": "AUTO",
  "cargoStatus": "IN_TRANSIT",
  "currentPosition": "Flur 1",
  "lastNode": "Mensa",
  "targetNode": "Zimmer 101"
}
```

### 2. Command Polling / Stream (Robot <-> Backend)

Damit der Roboter weiß, was er tun soll (z.B. wenn ein User `/routes/select` aufruft), braucht er einen Kanal.

- **WebSocket (Empfohlen für Echtzeit)**
- `ws://backend-server/ws/robot/control`
- Das Backend pusht hier Befehle direkt an den Roboter:
- `{ "command": "NAVIGATE", "start": "Mensa", "destination": "Zimmer 101" }`
- `{ "command": "CANCEL" }`
- `{ "command": "SET_MODE", "mode": "MANUAL" }`

### 3. Event-Meldungen (Robot -> Backend)

Spezifische Ereignisse, die eine Logik im Backend auslösen (z.B. Benachrichtigungen an User).

- `POST /table/event`
- **Payload**:

```json
{
  "event": "ARRIVAL_AT_START",
  "timestamp": "2023-10-27T10:00:00Z"
}
```

- **Events**: `ARRIVAL_AT_START`, `START_BUTTON_PRESSED`, `DESTINATION_REACHED`, `OBSTACLE_DETECTED`, `EMERGENCY_STOP`.

## Manual Control Bridge (WebSocket Relay)

Wenn ein User das `manualLock` hat, fungiert das Backend als **Relay (Brücke)**.

1. **User** sendet Steuerbefehle per WebSocket an **Backend**.
2. **Backend** prüft, ob der User das Lock hat.
3. **Backend** leitet die Befehle an den **Roboter-WebSocket** weiter.

### Datenstruktur für Fahrbefehle (Echtzeit):

```json
{
  "type": "DRIVE_COMMAND",
  "linear_velocity": 0.5, // Vorwärts/Rückwärts
  "angular_velocity": 0.2 // Drehung
}
```
