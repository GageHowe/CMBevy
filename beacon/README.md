# beacon

This is the centralized server that will serve multiple roles:
* Keep a sqlite database of users and handle authentication
* Listen for GameServers to request advertisement
* Display available GameServers available to Clients

## Account model

Beacon now separates:
* internal game accounts
* linked external identities (`steam`, `xbox`, `standalone`)
* login sessions

Gameplay should key off the internal `account_id`. Steam/Xbox/standalone are only auth providers that can link to that account.

## Environment

For Steam auth on the server:
* `STEAM_PUBLISHER_KEY` must be set
* `STEAM_APP_ID` defaults to `3526510`
* `STEAM_WEBAPI_IDENTITY` defaults to `beacon`

## API

### `POST /auth/register/standalone`

Create an internal account and link a standalone identity.

**Body:**
```json
{ "email": "pilot@example.com", "password": "hunter2hunter2", "display_name": "Pilot" }
```

### `POST /auth/login/standalone`

Login to an existing standalone-linked account.

**Body:**
```json
{ "email": "pilot@example.com", "password": "hunter2hunter2" }
```

### `POST /auth/login/provider`

Login or create an account through an external provider identity.

**Body:**
```json
{
  "provider": "steam",
  "provider_user_id": "",
  "display_name": "Pilot",
  "proof": "opaque-provider-proof"
}
```

`proof` is verified against Steam's `AuthenticateUserTicket` Web API. Beacon ignores the client-supplied `provider_user_id` and uses the verified SteamID returned by Steam.

### `POST /auth/link/provider`

Link another provider identity to an already-authenticated account.

**Body:**
```json
{
  "session_token": "sess_...",
  "provider": "steam",
  "provider_user_id": "76561198000000000",
  "proof": "opaque-provider-proof"
}
```

### `GET /auth/session/{token}`

Resolve the internal account for an existing login session.

### `POST /auth/logout`

Delete a login session.

### `POST /lobbies/register`

Called by a game server to advertise itself. The beacon derives the host IP from the requester's address.

**Body:**
```json
{ "quic_port": 42070, "name": "My Lobby", "max_players": 8 }
```

**Response:**
```json
{ "id": "1772405656705249800" }
```

**curl:**
```
curl -X POST http://localhost:8000/lobbies/register -H "Content-Type: application/json" -d "{\"quic_port\":42070,\"name\":\"My Lobby\",\"max_players\":8}"
```

### `GET /lobbies/partial`

Returns an HTMX HTML partial listing all active lobbies.
