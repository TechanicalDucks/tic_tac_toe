#[tokio::main]
async fn main() {
    rust_tic_tac_toe_game_server::server::start_server().await;
}