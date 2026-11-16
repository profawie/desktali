//! Mesej yang dihantar antara server dan client.
//! Setiap mesej ialah satu baris JSON (dipisahkan dengan '\n').

use rdev::EventType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Msg {
    /// Client -> server sebaik sahaja bersambung: saiz skrin dan OS client.
    Hello { width: f64, height: f64, os: String },
    /// Server -> client: letak kursor pada kedudukan mutlak ini.
    MoveTo { x: f64, y: f64 },
    /// Server -> client: butang mouse, scroll atau kekunci keyboard.
    Input(EventType),
    /// Dua hala: teks clipboard terkini.
    Clipboard(String),
}

impl Msg {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = serde_json::to_vec(self).expect("serialize msg");
        v.push(b'\n');
        v
    }

    pub fn decode(line: &str) -> Option<Msg> {
        serde_json::from_str(line).ok()
    }
}
