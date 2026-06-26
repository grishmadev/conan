use std::default;

pub enum PeerStatus {
    Connected,
    #[default]
    NotFound,
}

#[non_exhaustive]
pub enum Msg {
    Text(String),
    Begin,
    End,
}

impl From<&str> for Msg {
    fn from(value: &str) -> Self {
        Msg::Text(value.to_string())
    }
}
