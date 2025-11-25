use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerMark {
    X,
    O,
}

impl PlayerMark {
    pub fn to_string(&self) -> String {
        match self {
            PlayerMark::X => "x".to_string(),
            PlayerMark::O => "o".to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct RoomStateResponse {
    pub room_id: String,
    pub num_connections: usize,
    pub message: String,
    pub success: bool,
    pub my_mark: String,
}

impl RoomStateResponse {
    pub fn to_json_value(&self) -> Value {
        serde_json::to_value(self).unwrap()
    }
}

#[derive(Serialize)]
pub struct GameStateResponse {
    pub room_id: String,
    pub board: Vec<Vec<Option<String>>>,
    pub current_turn: Option<String>,
    pub winner: Option<String>,
    pub started: bool,
    pub moves_count: u8,
}

impl GameStateResponse {
    pub fn to_json_value(&self) -> Value { serde_json::to_value(self).unwrap() }
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub room_id: String,
    pub code: String,
    pub message: String,
}

impl ErrorResponse { pub fn to_json_value(&self) -> Value { serde_json::to_value(self).unwrap() } }

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseType {
    RoomState,
    GameState,
    Error,
}

#[derive(Serialize)]
pub struct RoomResponse {
    pub response_type: ResponseType,
    pub response: Value,
}
