use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug)]
pub enum PlayerMark {
    X,
    O,
}

impl PlayerMark {
    pub fn to_string(&self) -> String {
        match self {
            PlayerMark::X => "X".to_string(),
            PlayerMark::O => "O".to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct RoomStateResponse {
    pub room_id: String,
    pub num_connections: usize,
    pub message: String,
    pub success: bool,
    pub my_mark: PlayerMark,
}
