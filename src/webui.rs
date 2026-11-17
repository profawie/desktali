//! Local web page for browsing the other computer's files.
//!
//! A tiny HTTP server listens on 127.0.0.1 only (random port) and every request must
//! carry a random token that is only put in the URL opened in this computer's browser.
//! The page asks this app, which forwards the request to the other computer over the
//! existing rtetherfy connection.

use crate::files::{self, os_label, MAX_CHUNK};
use crate::protocol::{FsData, FsOp};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

const PAGE: &str = include_str!("files.html");
/// Listing can take a while the first time (approval prompt, macOS permission prompt).
const LIST_TIMEOUT: Duration = Duration::from_secs(130);
const CHUNK_TIMEOUT: Duration = Duration::from_secs(60);

static SERVER: OnceLock<(u16, String)> = OnceLock::new();

/// Start the local server (once) and open the page in the default browser.
pub fn open() {
    let (port, token) = match SERVER.get() {
        Some(s) => s.clone(),
        None => match start() {
            Ok(s) => SERVER.get_or_init(|| s).clone(),
            Err(e) => {
                log!("[files] cannot start the local file browser: {e}");
                return;
            }
        },
    };
    let url = format!("http://127.0.0.1:{port}/?t={token}");
    log!("[files] opening the file browser on port {port}");
    open_path(&url);
}

fn start() -> Result<(u16, String), String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = random_token();
    let t = token.clone();
    thread::spawn(move || {
        for s in listener.incoming().flatten() {
            let t = t.clone();
            thread::spawn(move || {
                if let Err(e) = handle(s, port, &t) {
                    log!("[files] request failed: {e}");
                }
            });
        }
    });
    Ok((port, token))
}

fn random_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    (0..2u64)
        .map(|i| {
            // RandomState is seeded from the OS random number generator.
            let mut h = RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(nanos);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

struct Req {
    method: String,
    path: String,
    query: Vec<(String, String)>,
}

impl Req {
    fn get(&self, k: &str) -> Option<&str> {
        self.query.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }
}

fn handle(mut s: TcpStream, port: u16, token: &str) -> std::io::Result<()> {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let mut host = String::new();
    let mut content_len = 0usize;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "host" => host = v.trim().to_ascii_lowercase(),
                "content-length" => content_len = v.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    if content_len > 0 && content_len < 65536 {
        let mut body = vec![0; content_len];
        let _ = r.read_exact(&mut body);
    }

    // Only this computer, and no DNS rebinding: the Host must be our own address.
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return respond(&mut s, 403, "text/plain", b"Forbidden");
    }
    let (path, qs) = target.split_once('?').unwrap_or((&target, ""));
    let req = Req {
        method,
        path: path.to_string(),
        query: qs
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (k, v) = p.split_once('=').unwrap_or((p, ""));
                (pct_decode(k), pct_decode(v))
            })
            .collect(),
    };
    if !same(req.get("t").unwrap_or(""), token) {
        return respond(
            &mut s,
            403,
            "text/plain",
            b"Forbidden: open this page from the rtetherfy menu (Files).",
        );
    }

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => respond(&mut s, 200, "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "/api/info") => {
            let peer = files::peer_os().map(|os| os_label(&os).to_string());
            let me = os_label(std::env::consts::OS).to_string();
            json(&mut s, &serde_json::json!({ "peer": peer, "me": me }))
        }
        ("GET", "/api/list") => {
            let p = req.get("p").unwrap_or("").to_string();
            match files::request(FsOp::List { path: p }, LIST_TIMEOUT) {
                Ok(FsData::List(l)) => json(&mut s, &serde_json::json!({ "ok": l })),
                Ok(_) => json(&mut s, &serde_json::json!({ "error": "Unexpected answer." })),
                Err(e) => json(&mut s, &serde_json::json!({ "error": e })),
            }
        }
        ("GET", "/api/file") => {
            let p = req.get("p").unwrap_or("").to_string();
            send_file(&mut s, &p, req.get("dl") == Some("1"))
        }
        ("POST", "/api/open") => {
            let p = req.get("p").unwrap_or("").to_string();
            match save_and_open(&p) {
                Ok(saved) => json(&mut s, &serde_json::json!({ "saved": saved })),
                Err(e) => json(&mut s, &serde_json::json!({ "error": e })),
            }
        }
        _ => respond(&mut s, 404, "text/plain", b"Not found"),
    }
}

/// Compare without stopping at the first different character.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pct_encode(s: &str) -> String {
    s.bytes()
        .map(|c| match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (c as char).to_string(),
            _ => format!("%{c:02X}"),
        })
        .collect()
}

fn head(s: &mut TcpStream, code: u16, ctype: &str, len: u64, extra: &str) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    write!(
        s,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {len}\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\n\
         X-Frame-Options: DENY\r\n{extra}Connection: close\r\n\r\n"
    )
}

fn respond(s: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    head(s, code, ctype, body.len() as u64, "")?;
    s.write_all(body)
}

fn json(s: &mut TcpStream, v: &serde_json::Value) -> std::io::Result<()> {
    respond(s, 200, "application/json", v.to_string().as_bytes())
}

/// Last part of a path from the other computer (which may use '\' or '/').
fn file_name(p: &str) -> String {
    p.rsplit(['/', '\\'])
        .find(|x| !x.is_empty())
        .unwrap_or("file")
        .to_string()
}

fn content_type(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "log" | "md" | "csv" | "ini" | "cfg" | "conf" | "yml" | "yaml" | "toml" | "rs" | "py"
        | "js" | "ts" | "c" | "h" | "cpp" | "java" | "go" | "sh" | "ps1" | "bat" | "sql" | "css" => {
            "text/plain; charset=utf-8"
        }
        "json" => "application/json",
        "html" | "htm" => "text/html; charset=utf-8",
        "xml" => "text/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn read_chunk(path: &str, offset: u64) -> Result<(Vec<u8>, u64), String> {
    match files::request(
        FsOp::Read {
            path: path.to_string(),
            offset,
            len: MAX_CHUNK,
        },
        CHUNK_TIMEOUT,
    )? {
        FsData::Chunk { data_b64, size } => {
            let data = B64.decode(data_b64).map_err(|e| e.to_string())?;
            Ok((data, size))
        }
        _ => Err("Unexpected answer.".into()),
    }
}

/// Stream a remote file to the browser, chunk by chunk.
fn send_file(s: &mut TcpStream, path: &str, download: bool) -> std::io::Result<()> {
    let (first, size) = match read_chunk(path, 0) {
        Ok(v) => v,
        Err(e) => {
            let page = format!(
                "<!doctype html><meta charset=utf-8><title>rtetherfy</title>\
                 <body style=\"font-family:system-ui;padding:24px\"><h3>Cannot open the file</h3><p>{}</p>",
                html_escape(&e)
            );
            return respond(s, 200, "text/html; charset=utf-8", page.as_bytes());
        }
    };
    let name = file_name(path);
    let ctype = if download { "application/octet-stream" } else { content_type(&name) };
    let disp = if download || ctype == "application/octet-stream" { "attachment" } else { "inline" };
    // Remote HTML/SVG must not run scripts on this page's origin.
    let sandbox = if ctype.starts_with("text/html") || ctype.contains("svg") || ctype.contains("xml") {
        "Content-Security-Policy: sandbox\r\n"
    } else {
        ""
    };
    let extra = format!(
        "Content-Disposition: {disp}; filename*=UTF-8''{}\r\n{sandbox}",
        pct_encode(&name)
    );
    head(s, 200, ctype, size, &extra)?;
    let mut sent = first.len() as u64;
    s.write_all(&first)?;
    while sent < size {
        let (data, _) = read_chunk(path, sent).map_err(std::io::Error::other)?;
        if data.is_empty() {
            break;
        }
        // Never send more than announced (the file may have grown meanwhile).
        let n = (data.len() as u64).min(size - sent) as usize;
        s.write_all(&data[..n])?;
        sent += n as u64;
    }
    Ok(())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Download a remote file to Downloads/rtetherfy and open it with its default app.
fn save_and_open(path: &str) -> Result<String, String> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");
    let dir = home
        .map(|h| PathBuf::from(h).join("Downloads"))
        .unwrap_or_else(std::env::temp_dir)
        .join("rtetherfy");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;

    let name = sanitize(&file_name(path));
    let (stem, ext) = match name.rsplit_once('.') {
        Some((a, b)) if !a.is_empty() => (a.to_string(), format!(".{b}")),
        _ => (name.clone(), String::new()),
    };
    let mut target = dir.join(&name);
    let mut n = 1;
    while target.exists() {
        target = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }

    let part = target.with_file_name(format!(
        "{}.part",
        target.file_name().unwrap_or_default().to_string_lossy()
    ));
    let result = (|| {
        let mut f = std::fs::File::create(&part).map_err(|e| format!("Cannot write {}: {e}", part.display()))?;
        let mut offset = 0u64;
        loop {
            let (data, size) = read_chunk(path, offset)?;
            if data.is_empty() {
                break;
            }
            f.write_all(&data).map_err(|e| e.to_string())?;
            offset += data.len() as u64;
            if offset >= size {
                break;
            }
        }
        Ok::<(), String>(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
    log!("[files] saved {}", target.display());
    open_path(&target.to_string_lossy());
    Ok(target.to_string_lossy().into_owned())
}

/// A file name that is valid on this computer.
fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let s = s.trim_matches([' ', '.']).to_string();
    if s.is_empty() { "file".into() } else { s }
}

/// Open a URL or file with the default app.
fn open_path(p: &str) {
    #[cfg(windows)]
    let r = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", p])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
    };
    #[cfg(target_os = "macos")]
    let r = Command::new("open").arg(p).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let r = Command::new("xdg-open").arg(p).spawn();
    if let Err(e) = r {
        log!("[files] cannot open {p}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode() {
        assert_eq!(pct_decode("C%3A%5CUsers%5Ca+b"), "C:\\Users\\a b");
        assert_eq!(pct_decode("%E2%9C%93"), "✓");
        assert_eq!(pct_decode("100%"), "100%");
        assert_eq!(pct_decode("%4"), "%4");
    }

    #[test]
    fn names() {
        assert_eq!(file_name("C:\\Users\\me\\a.txt"), "a.txt");
        assert_eq!(file_name("/Users/me/b.pdf"), "b.pdf");
        assert_eq!(sanitize("a:b?.txt"), "a_b_.txt");
    }
}
