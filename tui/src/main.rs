use std::error::Error;

use conan::App;

fn main() -> Result<(), Box<dyn Error>> {
    let mut terminal = ratatui::init();
    let mut app = App::default();
    app.manage_terminal(&mut terminal)?;
    app.manage_keys()?;
    Ok(())
}
