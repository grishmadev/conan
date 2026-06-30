use std::{
    error::Error,
    io::Stdout,
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame, Terminal,
    layout::{Constraint, Direction, HorizontalAlignment, Layout},
    prelude::CrosstermBackend,
    symbols::border,
    text::Line,
    widgets::{Block, Borders, List, Padding},
};

use crate::{
    components::new_peer::render_new_peer_block,
    matches::{Screen, Tab, TuiCommand, get_tuicmd},
};

pub mod components;
pub mod matches;

pub struct App {
    pub timer: Instant,
    pub tab: Tab,
    pub active_screen: Screen,
    pub running: bool,
    pub input: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            timer: Instant::now(),
            tab: Tab::None,
            active_screen: Screen::None,
            running: true,
            input: String::new(),
        }
    }
}

impl App {
    /// # Errors
    pub fn manage_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> Result<(), Box<dyn Error>> {
        let now = Instant::now();
        let timer = Duration::from_secs(1);
        loop {
            if now.elapsed() > timer {
                break;
            }
            terminal.draw(|f| {
                let area = f.area();
                let rect = area
                    .centered_horizontally(Constraint::Length(30))
                    .centered_vertically(Constraint::Max(3));
                let block = Block::new()
                    .border_set(border::DOUBLE)
                    .borders(Borders::ALL);
                let line = Line::from("Welcome").alignment(HorizontalAlignment::Center);
                let line_rect = block.inner(rect);
                f.render_widget(line, line_rect);
                f.render_widget(block, rect);
            })?;
        }
        while self.running {
            terminal.draw(|f| {
                self.set_layout(f);
                self.render(f);
            })?;
            self.manage_keys()?;
        }
        Ok(())
    }

    fn set_layout(&self, f: &mut Frame<'_>) {
        let area = f.area();
        let main_block = Block::new()
            .title(Line::from(" Conan ").alignment(HorizontalAlignment::Center))
            .borders(Borders::NONE)
            .padding(Padding::new(1, 1, 0, 0));

        let inner_block = main_block.inner(area);

        let chunks = Layout::default()
            .constraints([Constraint::Min(30), Constraint::Percentage(100)])
            .spacing(1)
            .margin(1)
            .direction(Direction::Horizontal)
            .split(inner_block);

        let left_block = Block::new()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .title_top(" Contact ");

        let contact_list = List::default()
            .items(["John Doe", "Jennie"])
            .block(left_block.clone());

        // right chunk
        let right_block = Block::new()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .title_top(Line::from(" Chat ").alignment(HorizontalAlignment::Center));

        f.render_widget(contact_list, chunks[0]);
        f.render_widget(right_block, chunks[1]);
        f.render_widget(main_block, area);
    }

    fn render(&self, f: &mut Frame<'_>) {
        match self.active_screen {
            Screen::None => {}
            Screen::PeerInputScreen { ref input } => {
                render_new_peer_block(f, input, f.area());
            }
        }
    }

    /// # Errors
    pub fn manage_keys(&mut self) -> std::io::Result<()> {
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            match self.active_screen {
                Screen::None => match get_tuicmd(key) {
                    TuiCommand::Quit => {
                        self.running = false;
                    }
                    TuiCommand::Other(key) => match key.code {
                        KeyCode::Tab => {
                            self.next_tab();
                        }
                        KeyCode::Char('a') => {
                            self.active_screen = Screen::PeerInputScreen { input: "".into() }
                        }
                        _ => {}
                    },
                },
                Screen::PeerInputScreen { ref mut input } => match key.code {
                    KeyCode::Char(ch) => {
                        input.push(ch);
                        self.active_screen = Screen::PeerInputScreen {
                            input: input.clone(),
                        }
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        self.active_screen = Screen::PeerInputScreen {
                            input: input.to_string(),
                        }
                    }
                    KeyCode::Enter | KeyCode::Esc => {
                        self.active_screen = Screen::None;
                    }
                    _ => {}
                },
            }
        }
        Ok(())
    }
}

pub trait TerminalControl {
    fn next_tab(&mut self);
}

impl TerminalControl for App {
    fn next_tab(&mut self) {
        let new_tab = match self.tab {
            Tab::Contact => Tab::Chat,
            _ => Tab::Contact,
        };
        self.tab = new_tab;
    }
}
