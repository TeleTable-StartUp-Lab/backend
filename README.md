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
  "text": "<was du heute genau getan hast>",
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
    systemHealth: "OK", //could be "OK", "WARNING", "ERROR", "OFFLINE"
    batteryLevel: "0-100%",
    driveMode: "MANUAL/AUTO/IDLE",
    cargoStatus: "LOADING", // Status des transportierten Gegenstands: "LOADING", "IN_TRANSIT", "DELIVERY_CONFIRMED", "EMPTY"
    lastRoute: ["Mensa", "Zimmer 101"],
    position: "Zimmer 101....", // kann auch "UNKNOWN" sein falls manual drive aktiv ist
    // TODO: Positioning muss noch genauer spizifiziert werden/
    manualLockHolderName: "<name des lock hodlers>"
  }
  ```

- `GET /nodes`
  - Liste der waehlbaren routen 
  ```json
  {
    nodes: ["Mensa", "Zimmer 101", "Zimmer 102", "..."]
  }
  ```
- `POST /routes/select`
  - waehle eine route aus
  - nachdem der Tisch die Route bekommen hat begibt er sich zum start, idelt dort und geht dann sobald der start knopf auf dem roboter gedrueckt wurde zur destiniation. 
  - Falls der Roboter in der Zwischenzeit auf Manual geschaltet wurde geht er immer zum letzten ziel hin
  ```json
  {
    start: "Mensa",
    destination: "Zimmer 101"
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
