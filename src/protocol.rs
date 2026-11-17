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
    /// Both ways: ask the other computer for a folder listing or part of a file.
    FsReq { id: u64, op: FsOp },
    /// Both ways: answer to the FsReq with the same id.
    FsResp { id: u64, result: Result<FsData, String> },
}

/// Remote file browsing request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FsOp {
    /// List a folder. An empty path lists the starting points (drives, home folder...).
    List { path: String },
    /// Read up to `len` bytes of a file, starting at `offset`.
    Read { path: String, offset: u64, len: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FsData {
    List(FsListing),
    /// File bytes (base64) and the total file size. Empty data means end of file.
    Chunk { data_b64: String, size: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsListing {
    /// The listed path in the other computer's own format ("" = starting points).
    pub path: String,
    /// Path to go "up" to; None when already at the starting points.
    pub parent: Option<String>,
    pub entries: Vec<FsItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsItem {
    pub name: String,
    pub path: String,
    pub dir: bool,
    pub size: u64,
    /// Last modified, seconds since 1970 (0 = unknown).
    pub modified: u64,
    pub hidden: bool,
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
