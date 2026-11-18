//! Tiny logger: prints to stderr and, in tray mode, appends to a log file
//! next to the configuration file.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static FILE: OnceLock<Mutex<File>> = OnceLock::new();

pub fn path() -> std::path::PathBuf {
    crate::config::dir().join("desktali.log")
}

/// Start logging to the log file (truncated when it gets larger than 1 MB).
pub fn init_file() {
    let p = path();
    let _ = std::fs::create_dir_all(crate::config::dir());
    let big = std::fs::metadata(&p).map(|m| m.len() > 1_000_000).unwrap_or(false);
    let f = OpenOptions::new()
        .create(true)
        .append(!big)
        .write(true)
        .truncate(big)
        .open(&p);
    if let Ok(f) = f {
        let _ = FILE.set(Mutex::new(f));
    }
}

pub fn write(line: &str) {
    eprintln!("{line}");
    if let Some(f) = FILE.get() {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
        if let Ok(mut f) = f.lock() {
            let _ = writeln!(f, "{h:02}:{m:02}:{s:02} UTC {line}");
        }
    }
}

macro_rules! log {
    ($($arg:tt)*) => { $crate::log::write(&format!($($arg)*)) };
}
