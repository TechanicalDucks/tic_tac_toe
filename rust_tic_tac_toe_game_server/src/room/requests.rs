use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    MakeMove,
    StartGame,
    RestartGame,
}

#[derive(Deserialize)]
pub struct Payload {
    pub action: Action,
    pub move_payload: Option<MakeMovePayload>,
}

#[derive(Deserialize)]
pub struct MakeMovePayload {
    pub x: u8,
    pub y: u8,
}
