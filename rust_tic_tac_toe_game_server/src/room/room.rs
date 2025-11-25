use std::collections::HashSet;
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::extract::ws::{Message, WebSocket};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use crate::server::{Room, SharedState};
use crate::room::{RoomStateResponse, PlayerMark, RoomResponse, ResponseType, GameStateResponse, ErrorResponse};
use crate::room::requests::{Payload, Action};

pub async fn join_room(
    Path(room_id): Path<String>,
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_join_room(room_id, socket, state))
}

// Helper: build serialized board for GameStateResponse
fn serialize_board(board: &[[Option<PlayerMark>;3];3]) -> Vec<Vec<Option<String>>> {
    board.iter().map(|row| {
        row.iter().map(|cell| cell.map(|m| m.to_string())).collect()
    }).collect()
}

fn build_game_state(room_id: &str, room: &Room) -> GameStateResponse {
    GameStateResponse {
        room_id: room_id.to_string(),
        board: serialize_board(&room.board),
        current_turn: if room.started && room.winner.is_none() { Some(room.current_turn.to_string()) } else { None },
        winner: room.winner.map(|w| w.to_string()),
        started: room.started,
        moves_count: room.moves_count,
    }
}

async fn broadcast_game_state(state: SharedState, room_id: &str) {
    let (snapshot, game_state) = {
        let app_state = state.lock().await;
        if let Some(room) = app_state.rooms.get(room_id) {
            let gs = build_game_state(room_id, room);
            let mut senders = Vec::new();
            for (&cid, (tx_conn, _mark)) in &room.connections { senders.push((cid, tx_conn.clone())); }
            (senders, gs)
        } else { return; }
    };
    let payload = RoomResponse { response_type: ResponseType::GameState, response: game_state.to_json_value()};
    let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    let mut dead = Vec::new();
    for (cid, tx) in &snapshot { if tx.send(json.clone()).is_err() { dead.push(*cid); } }
    if !dead.is_empty() { cleanup_dead_connections(state.clone(), room_id, dead, None, None).await; }
}

async fn send_error(state: SharedState, room_id: &str, code: &str, message: &str, to_connection: crate::server::ConnectionId) {
    let tx_opt = {
        let app_state = state.lock().await;
        app_state.rooms.get(room_id).and_then(|r| r.connections.get(&to_connection).map(|(tx, _)| tx.clone()))
    };
    if let Some(tx) = tx_opt {
        let err = ErrorResponse { room_id: room_id.to_string(), code: code.to_string(), message: message.to_string() };
        let payload = RoomResponse { response_type: ResponseType::Error, response: err.to_json_value()};
        if tx.send(serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())).is_err() {
            cleanup_dead_connections(state.clone(), room_id, vec![to_connection], None, None).await;
        }
    }
}

// New helper: centralize dead-connection cleanup to avoid duplication and optionally announce a message
async fn cleanup_dead_connections(
    state: SharedState,
    room_id: &str,
    dead_connections: Vec<crate::server::ConnectionId>,
    announce: Option<String>,
    exclude: Option<Vec<crate::server::ConnectionId>>,
) {
    // If there's nothing to remove and nothing to announce, nothing to do.
    if dead_connections.is_empty() && announce.is_none() {
        return;
    }

    // Prepare set of ids to remove (start with provided dead_connections)
    let mut to_remove: Vec<crate::server::ConnectionId> = dead_connections.into_iter().collect();

    // Snapshot current senders for this room under the lock, then release the lock
    let mut senders: Vec<(crate::server::ConnectionId, mpsc::UnboundedSender<String>, PlayerMark)> = Vec::new();
    {
        let app_state = state.lock().await;
        if let Some(room) = app_state.rooms.get(room_id) {
            for (&cid, (tx, mark)) in &room.connections {
                senders.push((cid, tx.clone(), *mark));
            }
        }
    }

    // Apply exclusion if provided (e.g., don't send the leave announce to the leaving connection)
    if let Some(exclude_ids) = exclude {
        let exclude_set: HashSet<_> = exclude_ids.into_iter().collect();
        senders.retain(|(cid, _, _)| !exclude_set.contains(cid));
    }

    // If an announcement is requested, send it to the snapshot of senders (after exclusion)
    if let Some(msg) = announce {
        for (cid, tx, _mark) in &senders {
            if tx.send(msg.clone()).is_err() {
                to_remove.push(*cid);
            }
        }
    }

    // Remove any connections discovered dead (either initially provided or discovered while announcing)
    if !to_remove.is_empty() {
        let mut app_state = state.lock().await;
        if let Some(room) = app_state.rooms.get_mut(room_id) {
            for cid in to_remove {
                room.connections.remove(&cid);
            }
            if room.connections.is_empty() {
                app_state.rooms.remove(room_id);
            }
        }
    }
}

async fn handle_join_room(room_id: String, socket: WebSocket, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Register this connection in the shared room state and get its connection id and assigned mark.
    // Change: perform an atomic check+insert under the same lock so we never exceed 2 connections.
    let connection_opt_and_count_and_mark = {
        let mut app_state = state.lock().await;
        // First, read the current count without holding a mutable reference to a room entry.
        let current_count = app_state
            .rooms
            .get(&room_id)
            .map(|r| r.connections.len())
            .unwrap_or(0);

        if current_count >= 2 {
            // Room already full: return (None, current_count, dummy)
            (None, current_count, PlayerMark::X)
        } else {
            // Reserve an id, determine mark, then insert into the room.
            let id = app_state.next_connection_id;
            app_state.next_connection_id = app_state.next_connection_id.wrapping_add(1);

            let assigned_mark = if current_count == 0 {
                PlayerMark::X
            } else {
                PlayerMark::O
            };

            let room = app_state.rooms.entry(room_id.clone()).or_insert_with(Room::new);
            room.connections.insert(id, (tx.clone(), assigned_mark));
            (Some(id), current_count + 1, assigned_mark)
        }
    };

    // If insertion was rejected because the room is full, notify the joining socket and close it.
    if let (None, current_count, _mark) = connection_opt_and_count_and_mark {
        let room_state_response = RoomStateResponse {
            room_id: room_id.clone(),
            num_connections: current_count,
            message: "Room is full".to_string(),
            success: false,
            my_mark: PlayerMark::X.to_string(), // dummy
        };

        let payload = RoomResponse {
            response_type: ResponseType::RoomState,
            response: room_state_response.to_json_value(),
        };

        let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
        // Send the JSON message and then send a Close frame to terminate connection cleanly.
        let _ = sender.send(Message::Text(json.into())).await;
        let _ = sender.send(Message::Close(None)).await;
        return;
    }

    // Unwrap the assigned connection id and mark (we know it's Some because we returned earlier if None)
    let connection_id = connection_opt_and_count_and_mark.0.unwrap();

    // Broadcast join notification
    {
        let mut dead_connections = Vec::new();
        let mut snapshot: Vec<(crate::server::ConnectionId, mpsc::UnboundedSender<String>, PlayerMark)> = Vec::new();

        {
            let app_state = state.lock().await;
            if let Some(room) = app_state.rooms.get(&room_id) {
                for (&cid, (tx_conn, mark)) in &room.connections {
                    snapshot.push((cid, tx_conn.clone(), *mark));
                }
            }
        }

        for (cid, tx_conn, mark) in &snapshot {
            let room_state_response = RoomStateResponse {
                room_id: room_id.clone(),
                num_connections: snapshot.len(),
                message: "Someone joined the room".to_string(),
                success: true,
                my_mark: (*mark.to_string()).to_owned(),
            };

            let payload = RoomResponse {
                response_type: ResponseType::RoomState,
                response: room_state_response.to_json_value(),
            };

            let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
            if tx_conn.send(json).is_err() {
                dead_connections.push(*cid);
            }
        }

        if !dead_connections.is_empty() {
            cleanup_dead_connections(state.clone(), &room_id, dead_connections, None, None).await;
        }
    }

    // If second player joined, auto-start game
    {
        let mut should_start = false;
        {
            let mut app_state = state.lock().await;
            if let Some(room) = app_state.rooms.get_mut(&room_id) {
                if room.connections.len() == 2 && !room.started { should_start = room.start_game().is_ok(); }
            }
        }
        if should_start { broadcast_game_state(state.clone(), &room_id).await; }
    }

    let state_for_recv = state.clone();
    let room_id_for_recv = room_id.clone();
    let my_mark = connection_opt_and_count_and_mark.2;

    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender
                .send(Message::Text(msg.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            if msg.is_err() { break; }
            match msg {
                Ok(msg_result) => {
                    if let Message::Text(text) = msg_result {
                        // Parse JSON payload
                        let parsed: Result<Payload, _> = serde_json::from_str(&text);
                        match parsed {
                            Ok(payload) => {
                                match payload.action {
                                    Action::StartGame => {
                                        let mut ok = false; let mut err: Option<&'static str> = None;
                                        {
                                            let mut app_state = state_for_recv.lock().await;
                                            if let Some(room) = app_state.rooms.get_mut(&room_id_for_recv) {
                                                match room.start_game() { Ok(()) => ok = true, Err(e) => err = Some(e) }
                                            } else { err = Some("room_not_found"); }
                                        }
                                        if ok { broadcast_game_state(state_for_recv.clone(), &room_id_for_recv).await; }
                                        else if let Some(code) = err { send_error(state_for_recv.clone(), &room_id_for_recv, code, code, connection_id).await; }
                                    }
                                    Action::MakeMove => {
                                        if let Some(mp) = payload.move_payload {
                                            let mut res: Result<(), &'static str> = Ok(());
                                            {
                                                let mut app_state = state_for_recv.lock().await;
                                                if let Some(room) = app_state.rooms.get_mut(&room_id_for_recv) {
                                                    res = room.make_move(my_mark, mp.x, mp.y);
                                                } else { res = Err("room_not_found"); }
                                            }
                                            match res {
                                                Ok(()) => broadcast_game_state(state_for_recv.clone(), &room_id_for_recv).await,
                                                Err(code) => send_error(state_for_recv.clone(), &room_id_for_recv, code, code, connection_id).await,
                                            }
                                        } else {
                                            send_error(state_for_recv.clone(), &room_id_for_recv, "missing_move_payload", "missing_move_payload", connection_id).await;
                                        }
                                    }
                                }
                            }
                            Err(_e) => {
                                send_error(state_for_recv.clone(), &room_id_for_recv, "invalid_json", "invalid_json", connection_id).await;
                            }
                        }
                    }
                }
                Err(_) => {}
            }
        }
    });

    tokio::select! { _ = send_task => {}, _ = recv_task => {}, }

    // Send leave announcement to others (exclude leaving conn)
    {
        let mut dead_connections = Vec::new();
        let mut senders = Vec::new();

        {
            let app_state = state.lock().await;
            if let Some(room) = app_state.rooms.get(&room_id) {
                for (&cid, (tx_conn, mark)) in &room.connections {
                    if cid != connection_id {
                        senders.push((cid, tx_conn.clone(), *mark));
                    }
                }
            }
        }

        for (cid, tx_conn, mark) in &senders {
            // Build a per-recipient leave payload including their own mark
            let room_state_response = RoomStateResponse {
                room_id: room_id.clone(),
                num_connections: senders.len(),
                message: format!("Player {} left the room", mark.to_string()),
                success: true,
                my_mark: (*mark.to_string()).to_owned(),
            };
            let payload = RoomResponse {
                response_type: ResponseType::RoomState,
                response: room_state_response.to_json_value(),
            };

            let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
            if tx_conn.send(json).is_err() {
                dead_connections.push(*cid);
            }
        }

        if !dead_connections.is_empty() {
            cleanup_dead_connections(state.clone(), &room_id, dead_connections, None, None).await;
        }
    }

    let mut app_state = state.lock().await;
    if let Some(room) = app_state.rooms.get_mut(&room_id) {
        room.connections.remove(&connection_id);
        if room.connections.is_empty() { app_state.rooms.remove(&room_id); }
    }
}
