//! Two-way clipboard sync: text, images and copied files/folders.
//!
//! One thread owns the clipboard object. It:
//!  * watches the local clipboard (via the OS change counter) and sends new content
//!    to the other machine;
//!  * receives content from the other machine (via a channel) and puts it on the
//!    local clipboard. Received files are saved in a temporary folder and placed on
//!    the clipboard as files, so they can be pasted in Explorer / Finder.

use crate::protocol::{FileEntry, Msg};
use arboard::{Clipboard, ImageData};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const POLL: Duration = Duration::from_millis(400);
const MAX_TEXT: usize = 8 * 1024 * 1024;
/// Largest total size of copied files that is sent.
pub const MAX_FILES_BYTES: u64 = 100 * 1024 * 1024;
const MAX_FILE_COUNT: usize = 2000;

/// Start the clipboard thread. `out` is the channel to the network writer.
/// Returns a Sender for clipboard messages (Clipboard / Image / Files) received
/// from the other machine.
pub fn spawn(out: Sender<Msg>) -> Sender<Msg> {
    let (in_tx, in_rx) = channel::<Msg>();
    thread::spawn(move || {
        let mut cb = loop {
            match Clipboard::new() {
                Ok(c) => break c,
                Err(e) => {
                    log!("[clipboard] cannot open clipboard: {e}; retrying...");
                    thread::sleep(Duration::from_secs(2));
                }
            }
        };
        // Do not send whatever is already in the clipboard at startup.
        let mut last_seq = change_count();
        let mut last_text: Option<String> = cb.get_text().ok();

        loop {
            match in_rx.recv_timeout(POLL) {
                Ok(msg) => {
                    apply(&mut cb, msg, &mut last_text);
                    last_seq = change_count();
                }
                Err(RecvTimeoutError::Timeout) => {
                    let seq = change_count();
                    // Without a change counter, fall back to comparing text.
                    if seq.is_some() && seq == last_seq {
                        continue;
                    }
                    last_seq = seq;
                    if let Some(msg) = read_local(&mut cb, &mut last_text, seq.is_some()) {
                        let _ = out.send(msg);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    in_tx
}

/// Read what was just copied locally. Files win over text, text over image
/// (apps like Word put both text and a picture of it on the clipboard).
fn read_local(cb: &mut Clipboard, last_text: &mut Option<String>, changed: bool) -> Option<Msg> {
    if changed {
        if let Ok(paths) = cb.get().file_list() {
            if !paths.is_empty() {
                return match collect_files(&paths) {
                    Ok((entries, bytes)) => {
                        log!("[clipboard] sending {} file(s), {} KB", entries.len(), bytes / 1024);
                        Some(Msg::Files(entries))
                    }
                    Err(e) => {
                        log!("[clipboard] files not sent: {e}");
                        if cfg!(target_os = "macos") && e.contains("os error 1)") {
                            log!("[clipboard] macOS blocked reading these files. Allow desktali in System Settings > Privacy & Security > Full Disk Access (or Files and Folders), then Restart");
                        }
                        None
                    }
                };
            }
        }
    }

    if let Ok(text) = cb.get_text() {
        if !text.is_empty() {
            if text.len() > MAX_TEXT || last_text.as_deref() == Some(text.as_str()) {
                return None;
            }
            *last_text = Some(text.clone());
            log!("[clipboard] sending {} chars", text.chars().count());
            return Some(Msg::Clipboard(text));
        }
    }

    if changed {
        if let Ok(img) = cb.get_image() {
            match encode_png(&img) {
                Ok(png) => {
                    log!(
                        "[clipboard] sending image {}x{} ({} KB)",
                        img.width,
                        img.height,
                        png.len() / 1024
                    );
                    return Some(Msg::Image {
                        png_b64: B64.encode(png),
                    });
                }
                Err(e) => log!("[clipboard] cannot encode image: {e}"),
            }
        }
    }
    None
}

fn apply(cb: &mut Clipboard, msg: Msg, last_text: &mut Option<String>) {
    match msg {
        Msg::Clipboard(text) => {
            if last_text.as_deref() != Some(text.as_str()) {
                match cb.set_text(text.clone()) {
                    Ok(()) => log!("[clipboard] received {} chars", text.chars().count()),
                    Err(e) => log!("[clipboard] write failed: {e}"),
                }
                *last_text = Some(text);
            }
        }
        Msg::Image { png_b64 } => {
            let r = B64
                .decode(png_b64)
                .map_err(|e| e.to_string())
                .and_then(|png| image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|e| e.to_string()))
                .and_then(|img| {
                    let rgba = img.to_rgba8();
                    let (width, height) = (rgba.width() as usize, rgba.height() as usize);
                    cb.set_image(ImageData {
                        width,
                        height,
                        bytes: Cow::Owned(rgba.into_raw()),
                    })
                    .map(|_| (width, height))
                    .map_err(|e| e.to_string())
                });
            match r {
                Ok((w, h)) => log!("[clipboard] received image {w}x{h}"),
                Err(e) => log!("[clipboard] cannot set image: {e}"),
            }
            *last_text = None;
        }
        Msg::Files(entries) => {
            match save_files(&entries).and_then(|top| {
                set_file_list(cb, &top)?;
                Ok(top)
            }) {
                Ok(top) => log!(
                    "[clipboard] received {} file(s); {} item(s) ready to paste",
                    entries.len(),
                    top.len()
                ),
                Err(e) => log!("[clipboard] cannot receive files: {e}"),
            }
            *last_text = None;
        }
        _ => {}
    }
}

fn encode_png(img: &ImageData) -> Result<Vec<u8>, String> {
    let buf = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.to_vec())
        .ok_or("bad image size")?;
    let mut out = std::io::Cursor::new(Vec::new());
    buf.write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// Read the copied files (folders recursively) into protocol entries.
fn collect_files(paths: &[PathBuf]) -> Result<(Vec<FileEntry>, u64), String> {
    fn walk(
        path: &Path,
        rel: String,
        out: &mut Vec<FileEntry>,
        total: &mut u64,
    ) -> Result<(), String> {
        if out.len() >= MAX_FILE_COUNT {
            return Err(format!("more than {MAX_FILE_COUNT} files"));
        }
        let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if meta.file_type().is_symlink() {
            return Ok(());
        }
        if meta.is_dir() {
            out.push(FileEntry {
                path: rel.clone(),
                dir: true,
                data_b64: String::new(),
            });
            let mut children: Vec<_> = std::fs::read_dir(path)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .filter_map(|e| e.ok())
                .collect();
            children.sort_by_key(|e| e.file_name());
            for c in children {
                let name = c.file_name().to_string_lossy().to_string();
                walk(&c.path(), format!("{rel}/{name}"), out, total)?;
            }
        } else {
            *total += meta.len();
            if *total > MAX_FILES_BYTES {
                return Err(format!(
                    "larger than {} MB in total",
                    MAX_FILES_BYTES / 1024 / 1024
                ));
            }
            let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            out.push(FileEntry {
                path: rel,
                dir: false,
                data_b64: B64.encode(data),
            });
        }
        Ok(())
    }

    let mut out = Vec::new();
    let mut total = 0;
    for p in paths {
        let Some(name) = p.file_name() else { continue };
        walk(p, name.to_string_lossy().to_string(), &mut out, &mut total)?;
    }
    if out.is_empty() {
        return Err("nothing to send".into());
    }
    Ok((out, total))
}

/// Folder where received files are stored. Older batches are removed.
pub fn received_dir() -> PathBuf {
    std::env::temp_dir().join("desktali-clipboard")
}

/// Write received entries to a new folder; return the top-level items.
fn save_files(entries: &[FileEntry]) -> Result<Vec<PathBuf>, String> {
    let root = received_dir();
    // Keep only the newest batch around.
    if let Ok(old) = std::fs::read_dir(&root) {
        for e in old.flatten() {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let base = root.join(stamp.to_string());
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;

    let mut top = Vec::new();
    for e in entries {
        // Only plain relative names: no "..", no absolute paths, no drive letters.
        let mut target = base.clone();
        for part in e.path.split('/') {
            let comp = Path::new(part).components().collect::<Vec<_>>();
            match comp.as_slice() {
                [Component::Normal(n)] => target.push(n),
                _ => return Err(format!("unsafe path in received files: {}", e.path)),
            }
        }
        if e.dir {
            std::fs::create_dir_all(&target).map_err(|err| err.to_string())?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
            }
            let data = B64.decode(&e.data_b64).map_err(|err| err.to_string())?;
            std::fs::write(&target, data).map_err(|err| err.to_string())?;
        }
        if !e.path.contains('/') {
            top.push(target);
        }
    }
    Ok(top)
}

/// Put files on the clipboard so they can be pasted in Explorer / Finder.
#[cfg(not(windows))]
fn set_file_list(cb: &mut Clipboard, paths: &[PathBuf]) -> Result<(), String> {
    cb.set().file_list(paths).map_err(|e| e.to_string())
}

/// Windows: write CF_HDROP ourselves. arboard 3.6.1 does not empty the clipboard
/// first, so Explorer could still see the old content. Also mark it as a copy
/// ("Preferred DropEffect") so Explorer pastes instead of moving.
#[cfg(windows)]
fn set_file_list(_cb: &mut Clipboard, paths: &[PathBuf]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use winapi::um::winbase::{GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
    use winapi::um::winuser::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
        CF_HDROP,
    };

    // DROPFILES header, then each path as UTF-16 + NUL, then a final NUL.
    let mut wide: Vec<u16> = Vec::new();
    for p in paths {
        wide.extend(p.as_os_str().encode_wide());
        wide.push(0);
    }
    wide.push(0);
    // DROPFILES { pFiles: u32, pt: (i32, i32), fNC: i32, fWide: i32 } = 20 bytes.
    const HEADER: usize = 20;

    unsafe fn global_with(bytes: &[u8]) -> Result<*mut winapi::ctypes::c_void, String> {
        let h = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len());
        if h.is_null() {
            return Err("GlobalAlloc failed".into());
        }
        let p = GlobalLock(h) as *mut u8;
        if p.is_null() {
            GlobalFree(h);
            return Err("GlobalLock failed".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        GlobalUnlock(h);
        Ok(h)
    }

    let mut drop = Vec::with_capacity(HEADER + wide.len() * 2);
    drop.extend_from_slice(&(HEADER as u32).to_le_bytes()); // pFiles
    drop.extend_from_slice(&[0u8; 12]); // pt.x, pt.y, fNC
    drop.extend_from_slice(&1u32.to_le_bytes()); // fWide
    for w in &wide {
        drop.extend_from_slice(&w.to_le_bytes());
    }

    unsafe {
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(null_mut()) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !opened {
            return Err("cannot open the clipboard".into());
        }
        let result = (|| {
            if EmptyClipboard() == 0 {
                return Err("EmptyClipboard failed".to_string());
            }
            let h = global_with(&drop)?;
            if SetClipboardData(CF_HDROP, h).is_null() {
                GlobalFree(h);
                return Err("SetClipboardData(CF_HDROP) failed".to_string());
            }
            let name: Vec<u16> = "Preferred DropEffect\0".encode_utf16().collect();
            let fmt = RegisterClipboardFormatW(name.as_ptr());
            if fmt != 0 {
                // DROPEFFECT_COPY = 1
                let h = global_with(&1u32.to_le_bytes())?;
                if SetClipboardData(fmt, h).is_null() {
                    GlobalFree(h);
                }
            }
            Ok(())
        })();
        CloseClipboard();
        result
    }
}

/// The OS clipboard change counter, so content is only read after a real copy.
#[cfg(windows)]
fn change_count() -> Option<u64> {
    Some(unsafe { winapi::um::winuser::GetClipboardSequenceNumber() } as u64)
}

#[cfg(target_os = "macos")]
fn change_count() -> Option<u64> {
    let n = objc2_app_kit::NSPasteboard::generalPasteboard().changeCount();
    Some(n as u64)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn change_count() -> Option<u64> {
    None
}
