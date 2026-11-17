//! Messages exchanged between server and client.
//! Each message is one line of JSON terminated by '\n'.

use rdev::EventType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Msg {
    /// Client -> server right after connecting: client screen size and OS.
    Hello { width: f64, height: f64, os: String },
    /// Server -> client: put the cursor at this absolute position.
    MoveTo { x: f64, y: f64 },
    /// Server -> client: mouse button, scroll (in wheel notches) or key.
    Input(EventType),
    /// Both ways: latest clipboard text.
    Clipboard(String),
    /// Both ways: clipboard image as base64 PNG.
    Image { png_b64: String },
    /// Both ways: copied files/folders. Paths are relative and '/'-separated;
    /// entries without '/' are the top-level items that were copied.
    Files(Vec<FileEntry>),
    /// Server -> client right after accepting: proves the other side is a rtetherfy server.
    Welcome { os: String },
    /// Both ways every 2 s, so a dead connection is noticed within a few seconds.
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub dir: bool,
    /// File contents, base64 (empty for folders).
    pub data_b64: String,
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
