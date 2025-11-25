use axum::routing::get;
use axum::Router;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use crate::room;

// A simple identifier for each WebSocket connection.
pub(crate) type ConnectionId = u64;

pub struct Room {
    // Map of connection id -> (sender channel for that connection, assigned PlayerMark)
    pub connections: HashMap<ConnectionId, (mpsc::UnboundedSender<String>, crate::room::PlayerMark)>,
    pub board: [[Option<crate::room::PlayerMark>; 3]; 3],
    pub started: bool,
    pub current_turn: crate::room::PlayerMark,
    pub winner: Option<crate::room::PlayerMark>,
    pub moves_count: u8,
}

impl Room {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            board: [[None; 3]; 3],
            started: false,
            current_turn: crate::room::PlayerMark::X,
            winner: None,
            moves_count: 0,
        }
    }

    pub fn start_game(&mut self) -> Result<(), &'static str> {
        if self.started {
            return Err("game_already_started");
        }
        if self.connections.len() < 2 {
            return Err("not_enough_players");
        }
        self.started = true;
        self.current_turn = crate::room::PlayerMark::X;
        self.winner = None;
        self.moves_count = 0;
        self.board = [[None; 3]; 3];
        Ok(())
    }

    pub fn make_move(&mut self, player: crate::room::PlayerMark, x: u8, y: u8) -> Result<(), &'static str> {
        if !self.started {
            return Err("game_not_started");
        }
        if self.winner.is_some() {
            return Err("game_already_finished");
        }
        if player != self.current_turn {
            return Err("not_your_turn");
        }
        if x > 2 || y > 2 {
            return Err("out_of_bounds");
        }
        let xi = x as usize;
        let yi = y as usize;
        if self.board[yi][xi].is_some() {
            return Err("cell_occupied");
        }
        self.board[yi][xi] = Some(player);
        self.moves_count += 1;
        // Check winner or draw
        if let Some(winner) = Self::check_winner(&self.board) {
            self.winner = Some(winner);
        } else if self.moves_count >= 9 {
            // draw -> winner stays None but game considered finished
        } else {
            self.current_turn = match self.current_turn {
                crate::room::PlayerMark::X => crate::room::PlayerMark::O,
                crate::room::PlayerMark::O => crate::room::PlayerMark::X,
            };
        }
        Ok(())
    }

    fn check_winner(board: &[[Option<crate::room::PlayerMark>; 3]; 3]) -> Option<crate::room::PlayerMark> {
        let lines = [
            // Rows
            [(0,0),(1,0),(2,0)],
            [(0,1),(1,1),(2,1)],
            [(0,2),(1,2),(2,2)],
            // Cols
            [(0,0),(0,1),(0,2)],
            [(1,0),(1,1),(1,2)],
            [(2,0),(2,1),(2,2)],
            // Diagonals
            [(0,0),(1,1),(2,2)],
            [(2,0),(1,1),(0,2)],
        ];
        for line in lines.iter() {
            let [a,b,c] = line;
            if let (Some(m1), Some(m2), Some(m3)) = (board[a.1][a.0], board[b.1][b.0], board[c.1][c.0]) {
                if m1 == m2 && m2 == m3 { return Some(m1); }
            }
        }
        None
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
        .route("/join/{room_id}", get(room::join_room))
        .with_state(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    axum::serve(listener, app).await.unwrap();
}