//! Configuration stored as JSON:
//!  * Windows: %APPDATA%\desktali\config.json
//!  * macOS:   ~/Library/Application Support/desktali/config.json

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// This machine has the physical mouse.
    Server,
    /// This machine is controlled by the server.
    Client,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];

    pub fn parse(s: &str) -> Option<Side> {
        match s.to_lowercase().as_str() {
            "left" => Some(Side::Left),
            "right" => Some(Side::Right),
            "top" => Some(Side::Top),
            "bottom" => Some(Side::Bottom),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Side::Left => "Left",
            Side::Right => "Right",
            Side::Top => "Top",
            Side::Bottom => "Bottom",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mode: Mode,
    /// Client mode: IP (or IP:port) of the server.
    pub server_address: String,
    /// Server mode: where the other machine's screen is.
    pub remote_side: Side,
    /// Server mode: TCP port to listen on.
    pub port: u16,
    /// Swap Ctrl <-> Cmd/Win when controlling a machine with a different OS.
    pub swap_ctrl_cmd: bool,
    /// Let the other computer browse and download this computer's files
    /// (each peer is still asked for once, see `file_browsing_peers`).
    pub allow_file_browsing: bool,
    /// Peers (IP, or server address on the client) allowed to browse files here.
    pub file_browsing_peers: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // Typical setup: the mouse is on Windows, the Mac is controlled.
            mode: if cfg!(target_os = "macos") { Mode::Client } else { Mode::Server },
            server_address: String::new(),
            remote_side: if cfg!(target_os = "macos") { Side::Left } else { Side::Right },
            port: crate::DEFAULT_PORT,
            swap_ctrl_cmd: true,
            allow_file_browsing: true,
            file_browsing_peers: Vec::new(),
        }
    }
}

impl Config {
    /// Server address with the default port added when missing.
    pub fn server_socket_addr(&self) -> String {
        let a = self.server_address.trim();
        if a.contains(':') {
            a.to_string()
        } else {
            format!("{a}:{}", crate::DEFAULT_PORT)
        }
    }

    pub fn load() -> Config {
        match std::fs::read_to_string(path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                log!("[config] invalid {}: {e}; using defaults", path().display());
                Config::default()
            }),
            Err(_) => {
                let c = Config::default();
                let _ = c.save();
                c
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path(), s).map_err(|e| e.to_string())
    }
}

pub fn dir() -> PathBuf {
    base_dir().join("desktali")
}

fn base_dir() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"));
    base.unwrap_or_else(|| PathBuf::from("."))
}

pub fn path() -> PathBuf {
    dir().join("config.json")
}
