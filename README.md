# TODO

- [ ] User login & Management
- [ ] Tagebuch- Funktionalitäten (einfügen, ändern, löschen) evtl. MD support

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
