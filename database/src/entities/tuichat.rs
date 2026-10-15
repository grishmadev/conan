use bincode::{Decode, Encode};
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Decode, Encode, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuiChat {
    pub id: u16,
    pub sender_name: String,
    pub data: String,
    pub time: String,
}

impl TuiChat {
    /// Builds a `TuiChat` struct with current timestamp
    #[must_use]
    pub fn build(text: &str, name: &str) -> Self {
        let time = Utc::now().to_string();
        Self {
            id: 0,
            sender_name: name.into(),
            data: text.into(),
            time,
        }
    }
}
