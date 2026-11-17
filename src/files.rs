//! Remote file browsing over the existing connection.
//!
//! The browsing side sends `FsReq` messages and waits for the matching `FsResp`
//! (see `request`). The browsed side answers them (see `on_request`), but only
//! after the user approved that peer once on the browsed computer.

use crate::config::Config;
use crate::net::Link;
use crate::protocol::{FsData, FsItem, FsListing, FsOp, Msg};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

/// Largest file chunk sent in one message (keeps the mouse responsive meanwhile).
pub const MAX_CHUNK: u64 = 512 * 1024;
/// Folders with more entries are cut off (the listing says so in the log).
const MAX_ENTRIES: usize = 20_000;

static LINK: OnceLock<Arc<Link>> = OnceLock::new();
/// The connected peer: (approval key, OS name such as "macos" / "windows").
static PEER: Mutex<Option<(String, String)>> = Mutex::new(None);
static PENDING: Mutex<Option<HashMap<u64, Sender<Result<FsData, String>>>>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Called once by the server / client with their connection.
pub fn attach(link: Arc<Link>) {
    let _ = LINK.set(link);
}

/// Called when the other computer finished the handshake (Some) or disconnected (None).
pub fn set_peer(peer: Option<(String, String)>) {
    *PEER.lock().unwrap() = peer;
    if PEER.lock().unwrap().is_none() {
        // Fail everything still waiting instead of letting it time out.
        if let Some(map) = PENDING.lock().unwrap().as_mut() {
            for (_, tx) in map.drain() {
                let _ = tx.send(Err("The connection to the other computer was lost.".into()));
            }
        }
    }
}

/// OS of the connected peer, if any.
pub fn peer_os() -> Option<String> {
    PEER.lock().unwrap().as_ref().map(|p| p.1.clone())
}

/// "Mac" / "Windows" for an OS name from `std::env::consts::OS`.
pub fn os_label(os: &str) -> &str {
    match os {
        "macos" => "Mac",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

/// Menu label for the remote file browser on this computer.
pub fn menu_label() -> &'static str {
    if cfg!(windows) {
        "Files (Mac)..."
    } else if cfg!(target_os = "macos") {
        "Files (Windows)..."
    } else {
        "Files (other computer)..."
    }
}

// ---------------------------------------------------------------- browsing side

/// Send a request to the other computer and wait for its answer.
pub fn request(op: FsOp, timeout: Duration) -> Result<FsData, String> {
    let Some(link) = LINK.get() else {
        return Err("rtetherfy is not running as server or client.".into());
    };
    let Some(os) = peer_os() else {
        return Err("Not connected to the other computer. Check the rtetherfy icon on both computers.".into());
    };
    let other = os_label(&os).to_string();
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = channel();
    PENDING
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(id, tx);
    if !link.send(&Msg::FsReq { id, op }) {
        forget(id);
        return Err("Not connected to the other computer.".into());
    }
    let r = rx.recv_timeout(timeout);
    forget(id);
    match r {
        Ok(r) => r,
        Err(_) => Err(format!(
            "No answer from the {other}. Check its screen: it may be asking you to allow access \
             (rtetherfy or macOS permission prompt). If the {other} runs an older rtetherfy, update it."
        )),
    }
}

fn forget(id: u64) {
    if let Some(map) = PENDING.lock().unwrap().as_mut() {
        map.remove(&id);
    }
}

/// Called by the read loop with an answer from the other computer.
pub fn on_reply(id: u64, result: Result<FsData, String>) {
    let tx = PENDING.lock().unwrap().as_mut().and_then(|m| m.remove(&id));
    if let Some(tx) = tx {
        let _ = tx.send(result);
    }
}

// ----------------------------------------------------------------- browsed side

/// Called by the read loop with a request from the other computer.
/// `peer_key` identifies the peer for the approval (its IP / configured address).
pub fn on_request(id: u64, op: FsOp, link: Arc<Link>, peer_key: String) {
    thread::spawn(move || {
        let result = approve(&peer_key).and_then(|_| run(op));
        link.send(&Msg::FsResp { id, result });
    });
}

#[derive(Clone, Copy)]
enum Answer {
    Asking,
    Allowed,
    Denied(Instant),
}

static ANSWERS: Mutex<Option<HashMap<String, Answer>>> = Mutex::new(None);

/// Ask the user on this computer once per peer; the answer "Allow" is saved in the config.
fn approve(peer_key: &str) -> Result<(), String> {
    let me = os_label(std::env::consts::OS).to_string();
    let cfg = Config::load();
    if !cfg.allow_file_browsing {
        return Err(format!(
            "File browsing is turned off on the {me} (\"allow_file_browsing\": false in its config file)."
        ));
    }
    if cfg.file_browsing_peers.iter().any(|p| p == peer_key) {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        {
            let mut g = ANSWERS.lock().unwrap();
            let map = g.get_or_insert_with(HashMap::new);
            match map.get(peer_key).copied() {
                Some(Answer::Allowed) => return Ok(()),
                // Debounce: requests right after "Deny" fail without asking again.
                Some(Answer::Denied(t)) if t.elapsed() < Duration::from_secs(10) => {
                    return Err(format!("Access was denied on the {me}. Reload to be asked again."));
                }
                Some(Answer::Asking) => {}
                _ => {
                    map.insert(peer_key.to_string(), Answer::Asking);
                    break;
                }
            }
        }
        if Instant::now() > deadline {
            return Err(format!("Nobody answered the access prompt on the {me}."));
        }
        thread::sleep(Duration::from_millis(200));
    }

    let who = match peer_os() {
        Some(os) => format!("The {} at {peer_key}", os_label(&os)),
        None => format!("The computer at {peer_key}"),
    };
    log!("[files] asking whether {peer_key} may browse files");
    let yes = confirm(&format!(
        "{who} wants to browse and download files on this computer through rtetherfy.\n\n\
         Allow it? The answer is remembered (remove it from \"file_browsing_peers\" in the config file to undo)."
    ));
    let answer = if yes { Answer::Allowed } else { Answer::Denied(Instant::now()) };
    ANSWERS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(peer_key.to_string(), answer);
    if !yes {
        log!("[files] file browsing denied for {peer_key}");
        return Err(format!("Access was denied on the {me}. Reload to be asked again."));
    }
    log!("[files] file browsing allowed for {peer_key}");
    let mut cfg = Config::load();
    if !cfg.file_browsing_peers.iter().any(|p| p == peer_key) {
        cfg.file_browsing_peers.push(peer_key.to_string());
        if let Err(e) = cfg.save() {
            log!("[files] cannot save config: {e}");
        }
    }
    Ok(())
}

/// Yes/No question on this computer's screen. Defaults to No.
fn confirm(text: &str) -> bool {
    #[cfg(windows)]
    {
        use winapi::um::winuser::{
            MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
        };
        let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (t, c) = (w(text), w("rtetherfy"));
        let r = unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                t.as_ptr(),
                c.as_ptr(),
                MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2 | MB_TOPMOST | MB_SETFOREGROUND,
            )
        };
        r == IDYES
    }
    #[cfg(target_os = "macos")]
    {
        let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "display dialog \"{}\" with title \"rtetherfy\" buttons {{\"Deny\", \"Allow\"}} \
             default button \"Deny\" with icon caution giving up after 110",
            q(text)
        );
        match std::process::Command::new("osascript").args(["-e", &script]).output() {
            Ok(o) => String::from_utf8_lossy(&o.stdout).contains("button returned:Allow"),
            Err(e) => {
                log!("[files] cannot show the access prompt: {e}");
                false
            }
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // No dialog available: only allow when explicitly listed in the config.
        log!("[files] access prompt not supported here; {text}");
        false
    }
}

fn run(op: FsOp) -> Result<FsData, String> {
    match op {
        FsOp::List { path } if path.is_empty() => Ok(FsData::List(roots())),
        FsOp::List { path } => list(&path).map(FsData::List),
        FsOp::Read { path, offset, len } => read(&path, offset, len),
    }
}

/// Explain an I/O error, with the fix for permission problems.
fn io_err(e: std::io::Error, path: &str) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            if cfg!(target_os = "macos") {
                format!(
                    "macOS blocked access to {path} ({e}). On the Mac, allow rtetherfy in System Settings > \
                     Privacy & Security > Full Disk Access (or Files and Folders), then choose Restart \
                     in the rtetherfy menu on the Mac."
                )
            } else if cfg!(windows) {
                format!(
                    "Windows denied access to {path} ({e}). The folder may be protected or belong to \
                     another user; rtetherfy can only open what your Windows account can open."
                )
            } else {
                format!("Permission denied: {path} ({e})")
            }
        }
        std::io::ErrorKind::NotFound => format!("{path} does not exist (anymore)."),
        _ => format!("{path}: {e}"),
    }
}

fn item(name: String, path: &Path) -> FsItem {
    // Follow symlinks (e.g. /tmp, /Volumes/Macintosh HD); fall back to the link itself.
    let md = std::fs::metadata(path).or_else(|_| std::fs::symlink_metadata(path));
    let (dir, size, modified, attr_hidden) = match &md {
        Ok(m) => (
            m.is_dir(),
            if m.is_dir() { 0 } else { m.len() },
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
            hidden_attr(m),
        ),
        Err(_) => (false, 0, 0, false),
    };
    FsItem {
        hidden: attr_hidden || name.starts_with('.'),
        name,
        path: path.to_string_lossy().into_owned(),
        dir,
        size,
        modified,
    }
}

#[cfg(windows)]
fn hidden_attr(m: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    m.file_attributes() & (HIDDEN | SYSTEM) != 0
}

#[cfg(not(windows))]
fn hidden_attr(_m: &std::fs::Metadata) -> bool {
    false
}

fn list(path: &str) -> Result<FsListing, String> {
    let p = PathBuf::from(path);
    if !p.is_absolute() {
        return Err(format!("Not a full path: {path}"));
    }
    let rd = std::fs::read_dir(&p).map_err(|e| io_err(e, path))?;
    let mut entries = Vec::new();
    for e in rd {
        let e = match e {
            Ok(e) => e,
            Err(err) => {
                log!("[files] skipping entry in {path}: {err}");
                continue;
            }
        };
        entries.push(item(e.file_name().to_string_lossy().into_owned(), &e.path()));
        if entries.len() >= MAX_ENTRIES {
            log!("[files] {path} has more than {MAX_ENTRIES} entries; list cut off");
            break;
        }
    }
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let parent = p
        .parent()
        .map(|q| q.to_string_lossy().into_owned())
        .filter(|q| !q.is_empty())
        .or_else(|| Some(String::new()));
    Ok(FsListing {
        path: p.to_string_lossy().into_owned(),
        parent,
        entries,
    })
}

/// Starting points: home folders and drives / volumes.
fn roots() -> FsListing {
    let mut entries = Vec::new();
    let mut add = |name: &str, p: PathBuf| {
        if p.is_dir() {
            let mut it = item(name.to_string(), &p);
            it.hidden = false;
            entries.push(it);
        }
    };
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(h) = home {
        add("Home", h.clone());
        for sub in ["Desktop", "Documents", "Downloads"] {
            add(sub, h.join(sub));
        }
    }
    #[cfg(windows)]
    for letter in b'A'..=b'Z' {
        let d = format!("{}:\\", letter as char);
        add(&format!("Drive {}:", letter as char), PathBuf::from(d));
    }
    #[cfg(not(windows))]
    {
        add("Computer (/)", PathBuf::from("/"));
        if let Ok(rd) = std::fs::read_dir("/Volumes") {
            for e in rd.flatten() {
                add(&e.file_name().to_string_lossy(), e.path());
            }
        }
    }
    FsListing {
        path: String::new(),
        parent: None,
        entries,
    }
}

fn read(path: &str, offset: u64, len: u64) -> Result<FsData, String> {
    let mut f = std::fs::File::open(path).map_err(|e| io_err(e, path))?;
    let md = f.metadata().map_err(|e| io_err(e, path))?;
    if md.is_dir() {
        return Err(format!("{path} is a folder."));
    }
    let size = md.len();
    let want = len.min(MAX_CHUNK).min(size.saturating_sub(offset));
    let mut buf = vec![0u8; want as usize];
    if want > 0 {
        f.seek(SeekFrom::Start(offset)).map_err(|e| io_err(e, path))?;
        let mut got = 0;
        while got < buf.len() {
            match f.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(io_err(e, path)),
            }
        }
        buf.truncate(got);
    }
    Ok(FsData::Chunk {
        data_b64: B64.encode(&buf),
        size,
    })
}
