# Tic Tac Toe Game Server (WebSocket API)

Lightweight Axum-based WebSocket server that hosts ephemeral 2‑player Tic Tac Toe rooms. Each room supports at most two concurrent connections. When the second player joins the room, the game auto-starts (unless already started manually).

Server default bind address: `0.0.0.0:3000`
WebSocket endpoint path: `/join/{room_id}`
Full WS URL example: `ws://localhost:3000/join/my-room-123`

---
## 1. Connecting
Perform a WebSocket handshake (HTTP GET with `Upgrade: websocket`) to `/join/{room_id}`.

- If the room does not exist, it is created automatically when the first client connects.
- If the room has 0 players, the joining player is assigned mark `x`.
- If the room has 1 player, the joining player is assigned mark `o`.
- If the room already has 2 players, the server sends a failure `room_state` response and immediately closes the connection.

### Example (JavaScript client)
```js
const ws = new WebSocket('ws://localhost:3000/join/test-room');
ws.onmessage = (ev) => console.log('Message:', ev.data);
ws.onopen = () => console.log('Connected');
ws.onclose = () => console.log('Closed');
```

---
## 2. Outgoing Responses From Server
All messages sent by the server are JSON objects wrapped in a common envelope:
```json
{
  "response_type": "room_state or game_state or error",
  "response": "object"
}
```
### 2.1 `room_state`
Sent:
- When you join (to everyone in the room, including yourself) with a message like "Someone joined the room".
- When a player leaves (to remaining players) with a message like "Player x left the room" or "Player o left the room".
- When a join attempt fails because the room is full (only to the rejected client, then the connection is closed).

Payload shape:
```json
{
  "room_id": "string",
  "num_connections": 1,               
  "message": "string",                
  "success": true,                   
  "my_mark": "x or o"                
}
```
Example successful join broadcast for player with mark `x`:
```json
{
  "response_type": "room_state",
  "response": {
    "room_id": "test-room",
    "num_connections": 1,
    "message": "Someone joined the room",
    "success": true,
    "my_mark": "x"
  }
}
```

### 2.2 `game_state`
Sent:
- Automatically when the second player joins (game auto-starts).
- After a valid move.
- After a successful manual start (`start_game` action).
- After a successful restart (`restart_game` action).

Payload shape:
```json
{
  "room_id": "string",
  "board": [[null,"x",null],["o",null,null],[null,null,null]],  
  "current_turn": "x or o or null",  
  "winner": "x or o or null",       
  "started": true,                     
  "moves_count": 3                    
}
```
Notes:
- Draw state: `winner` stays `null` but `current_turn` becomes `null` (game finished).
- Before start: `started` is false, `current_turn` is null, `board` all nulls.

Example game state update after a move:
```json
{
  "response_type": "game_state",
  "response": {
    "room_id": "test-room",
    "board": [["x",null,null],["o",null,null],[null,null,null]],
    "current_turn": "x",
    "winner": null,
    "started": true,
    "moves_count": 2
  }
}
```

### 2.3 `error`
Sent directly to the offending connection when an action fails (validation, sequence, or parsing).

Payload shape:
```json
{
  "room_id": "string",
  "code": "string",     
  "message": "string"  
}
```
Example:
```json
{
  "response_type": "error",
  "response": {
    "room_id": "test-room",
    "code": "not_your_turn",
    "message": "not_your_turn"
  }
}
```

#### Possible Error Codes
Action / flow errors:
- `room_not_found` (room disappeared mid-action)
- `game_already_started`
- `not_enough_players`
- `game_not_started`
- `game_already_finished`
- `not_your_turn`
- `out_of_bounds`
- `cell_occupied`
- `missing_move_payload`
- `restart_not_allowed` (attempted `restart_game` when game isn't finished or not enough players)
Parsing / protocol errors:
- `invalid_json`

---
## 3. Client -> Server Requests (Actions)
Clients send plain text WebSocket messages containing JSON request payloads.
Request schema:
```json
{
  "action": "start_game or make_move or restart_game",
  "move_payload": { "x": 0, "y": 2 } 
}
```
### 3.1 Start Game Manually
Optional (game auto-starts when second player joins). Only valid if:
- At least 2 players present
- Game not already started
```json
{ "action": "start_game" }
```
Responses:
- On success: a `game_state` broadcast to all players
- On failure: `error` (e.g., `game_already_started`, `not_enough_players`)

### 3.2 Make Move
Coordinates are 0-based indices: `x` is column (0..2), `y` is row (0..2).
```json
{ "action": "make_move", "move_payload": { "x": 1, "y": 2 } }
```
Responses:
- On success: updated `game_state` broadcast
- On failure: `error` (`game_not_started`, `not_your_turn`, `out_of_bounds`, `cell_occupied`, etc.)

#### Turn Logic
- First turn always belongs to `x`.
- Turns alternate after each successful non-terminal move.
- After win or draw, further `make_move` attempts yield `game_already_finished`.

### 3.3 Restart Game (new)
Clients can request a game restart using the `restart_game` action. This action is intended to restart a finished game (win or draw) without players disconnecting.

Request:
```json
{ "action": "restart_game" }
```

Rules & behavior (assumptions / server contract):
- Allowed only when the room currently has 2 connected players and the previous game has finished (either `winner` is set or `moves_count >= 9`).
- On success the server resets the board to all `null`, sets `started` to `true`, `current_turn` to `x`, `winner` to `null`, and `moves_count` to `0`.
- The server broadcasts a `game_state` update to all players reflecting the reset board and new turn order.
- If the request is sent while the game is still in progress or there are fewer than two players, the server responds with an `error` (code: `restart_not_allowed` or `not_enough_players`).

Responses:
- On success: `game_state` broadcast with a fresh game board and `current_turn = "x"`.
- On failure: `error` (e.g., `restart_not_allowed`, `not_enough_players`).

Notes for clients:
- If you want the ability to request a restart immediately after a finished game, call `restart_game` once you receive a `game_state` that has a `winner` or `current_turn` set to `null` (draw).
- Clients should handle the incoming `game_state` broadcast by resetting local UI state to the new board and current turn.

---
## 4. Lifecycle Example
1. Player A connects (`my_mark = "x"`). Receives `room_state`.
2. Player B connects (`my_mark = "o"`). Both players receive `room_state`, then auto `game_state` (empty board, `current_turn = "x"`).
3. Player A sends `make_move (0,0)`. All receive updated `game_state` (board[0][0] = "x", `current_turn = "o").
4. Player B sends `make_move (0,1)`. Broadcast `game_state`.
5. ... continue until win or draw.
6. After the game finishes (win or draw), either player may send `{ "action": "restart_game" }`.
   - If accepted, both players receive a fresh `game_state` and play resumes with `x` to move.
7. A player disconnects → remaining player receives a `room_state` leave message.

---
## 5. Board Representation
A 3x3 matrix of `null | "x" | "o"` serialized as an array of arrays. Outer array is rows (y), inner arrays are columns (x).
Example initial board:
```json
[[null,null,null],[null,null,null],[null,null,null]]
```

---
## 6. Closing Behavior
- On full-room rejection: server sends a `room_state` with `success=false` then a Close frame.
- A normal disconnect by a player triggers a `room_state` message to remaining players.
- If the last player leaves, the room is removed from memory.

---
## 7. Running the Server
From project root:
```bash
cargo run
```
Server listens on port 3000.

---
## 8. Versioning & Stability
This API is minimal and may evolve. Consider wrapping your client parsing with defensive checks (ignore unknown fields, handle missing optional ones).

---
## 9. Summary Cheat Sheet
Endpoint: `ws://<host>:3000/join/{room_id}`
Request Actions:
- `start_game`
- `make_move` with `{"x":0..2,"y":0..2}`
- `restart_game` (restart a finished game when both players are present)
Response Envelope: `{ "response_type": "room_state" | "game_state" | "error", "response": <object> }`
Key Error Codes: `not_your_turn`, `cell_occupied`, `out_of_bounds`, `game_not_started`, `game_already_finished`, `restart_not_allowed`, `invalid_json`, ...

---
Happy hacking!
