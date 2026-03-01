# beacon

This is the centralized server that will serve multiple roles:
* Keep a sqlite database of users and handle authentication
* Listen for GameServers to request advertisement
* Display available GameServers available to Clients

## API

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
