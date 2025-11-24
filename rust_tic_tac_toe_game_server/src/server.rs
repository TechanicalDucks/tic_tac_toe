use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Router};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

// A simple identifier for each WebSocket connection.
type ConnectionId = u64;

pub struct Room {
    // Map of connection id -> sender channel for that connection
    pub connections: HashMap<ConnectionId, mpsc::UnboundedSender<String>>,
}

impl Room {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }
}

pub struct AppState {
    pub rooms: HashMap<String, Room>,
    // Counter to assign unique connection ids
    pub next_connection_id: ConnectionId,
}

pub type SharedState = Arc<Mutex<AppState>>;

pub async fn start_server() {
    let state: SharedState = Arc::new(Mutex::new(AppState {
        rooms: HashMap::new(),
        next_connection_id: 0,
    }));

    let app = Router::new()
        .route("/join/{room_id}", get(join_room))
        .with_state(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    axum::serve(listener, app).await.unwrap();
}

pub async fn join_room(
    Path(room_id): Path<String>,
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_join_room(room_id, socket, state))
}

async fn handle_join_room(room_id: String, socket: WebSocket, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Register this connection in the shared room state and get its connection id.
    let connection_id = {
        let mut app_state = state.lock().await;
        let id = app_state.next_connection_id;
        app_state.next_connection_id = app_state.next_connection_id.wrapping_add(1);

        let room = app_state
            .rooms
            .entry(room_id.clone())
            .or_insert_with(Room::new);

        room.connections.insert(id, tx.clone());

        id
    };

    // Broadcast a join message to everyone in the room.
    {
        let mut dead_connections = Vec::new();
        let mut senders = Vec::new();

        {
            let app_state = state.lock().await;
            if let Some(room) = app_state.rooms.get(&room_id) {
                for (&cid, tx_conn) in &room.connections {
                    senders.push((cid, tx_conn.clone()));
                }
            }
        }

        for (cid, tx_conn) in &senders {
            if tx_conn
                .send("Someone joined the room".to_string())
                .is_err()
            {
                dead_connections.push(*cid);
            }
        }

        if !dead_connections.is_empty() {
            let mut app_state = state.lock().await;
            if let Some(room) = app_state.rooms.get_mut(&room_id) {
                for cid in dead_connections {
                    room.connections.remove(&cid);
                }
            }
        }
    }

    let state_for_recv = state.clone();
    let room_id_for_recv = room_id.clone();

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
            if msg.is_err() {
                break;
            }

            match msg {
                Ok(msg_result) => {
                    if let Message::Text(text) = msg_result {
                        // Broadcast this message to all connections in the same room
                        let mut dead_connections = Vec::new();
                        let mut senders = Vec::new();

                        {
                            let app_state = state_for_recv.lock().await;
                            if let Some(room) = app_state.rooms.get(&room_id_for_recv) {
                                for (&cid, tx_conn) in &room.connections {
                                    senders.push((cid, tx_conn.clone()));
                                }
                            }
                        }

                        for (cid, tx_conn) in &senders {
                            if tx_conn.send(text.clone().to_string()).is_err() {
                                dead_connections.push(*cid);
                            }
                        }

                        if !dead_connections.is_empty() {
                            let mut app_state = state_for_recv.lock().await;
                            if let Some(room) = app_state.rooms.get_mut(&room_id_for_recv) {
                                for cid in dead_connections {
                                    room.connections.remove(&cid);
                                }
                            }
                        }
                    }
                }
                Err(_) => {}
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    // On disconnect, remove this connection from the room
    let mut app_state = state.lock().await;
    if let Some(room) = app_state.rooms.get_mut(&room_id) {
        room.connections.remove(&connection_id);
        if room.connections.is_empty() {
            app_state.rooms.remove(&room_id);
        }
    }
}
