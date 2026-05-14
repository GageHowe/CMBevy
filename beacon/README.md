# beacon

This is the centralized server that will serve multiple roles:
* Keep a sqlite database of assets
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

### `POST /lobbies/{id}/heartbeat`

Update the advertised player counts for an existing lobby.

**Body:**
```json
{ "player_count": 3, "max_players": 8 }
```

**curl:**
```
curl -X POST https://your-beacon-domain.example/lobbies/register -H "Content-Type: application/json" -d "{\"quic_port\":42070,\"name\":\"My Lobby\",\"max_players\":8}"
```

### `GET /lobbies/partial`

Returns an HTMX HTML partial listing all active lobbies.

## Running

Local development:
```bash
cargo run -p beacon
```

That binds `127.0.0.1:8000`.

Production with Caddy:
```bash
cargo build -p beacon --release
cp beacon/Caddyfile /etc/caddy/Caddyfile
systemctl reload caddy
./target/release/beacon
```

Caddy handles HTTPS for `criticalmass.dev` and proxies to `http://127.0.0.1:8000`.
