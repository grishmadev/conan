use conan_chat::App;
use conanprotocol::config::parse_config;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?;
    let userid = config
        .socket_path
        .split('/')
        .next_back()
        .unwrap_or("Unknown User")
        .split('.')
        .next()
        .unwrap();
    let mut terminal = ratatui::init();
    let mut app = App::create(config.clone()).await?;
    app.manage_terminal(&mut terminal, userid).await?;

    Ok(())
}
