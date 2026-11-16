//! Server: the machine with the physical mouse.
//!
//! A platform input capture (`win_capture` / `mac_capture`) feeds events into [`Core`].
//! When the cursor reaches the configured screen edge, control "crosses" to the client:
//! local input is swallowed and forwarded. When the client's virtual cursor leaves
//! through the opposite edge, control returns to this machine.

use crate::config::{Config, Side};
use crate::net::{self, Link};
use crate::protocol::Msg;
use crate::status::{self, State};
use crate::{clipboard, local_ip, screen_size};
use rdev::{EventType, Key};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

#[cfg(target_os = "macos")]
use crate::mac_capture as capture;
#[cfg(not(target_os = "macos"))]
use crate::win_capture as capture;

/// How many pixels from the edge count as "at the edge".
const EDGE: f64 = 2.0;
/// When returning, place the cursor this far from the edge so it does not cross again.
const RETURN_MARGIN: f64 = 6.0;
/// macOS scroll pixels per mouse wheel notch.
const MAC_PIXELS_PER_NOTCH: i64 = 10;

pub struct Core {
    pub local: (f64, f64),
    side: Side,
    swap_ctrl_cmd: bool,
    /// Client screen size, when a client is connected.
    remote: Option<(f64, f64)>,
    remote_os: String,
    remote_name: String,
    /// Current connection number (to ignore old connections that close later).
    conn_gen: u64,
    /// True while the mouse/keyboard control the client.
    pub active: bool,
    /// Virtual cursor position on the client screen.
    rx: f64,
    ry: f64,
    /// macOS scroll leftover (pixels) not yet a whole notch.
    wheel_acc: (i64, i64),
    out: Sender<Msg>,
}

pub type SharedCore = Arc<Mutex<Core>>;

impl Core {
    /// Called with the local cursor position while not active.
    /// Returns true when control moved to the client.
    pub fn try_enter(&mut self, x: f64, y: f64) -> bool {
        let Some((rw, rh)) = self.remote else { return false };
        let (w, h) = self.local;
        let hit = match self.side {
            Side::Right => x >= w - EDGE,
            Side::Left => x <= EDGE - 1.0,
            Side::Bottom => y >= h - EDGE,
            Side::Top => y <= EDGE - 1.0,
        };
        if !hit {
            return false;
        }
        let fy = (y / h).clamp(0.0, 1.0);
        let fx = (x / w).clamp(0.0, 1.0);
        let (rx, ry) = match self.side {
            Side::Right => (1.0, fy * rh),
            Side::Left => (rw - 2.0, fy * rh),
            Side::Bottom => (fx * rw, 1.0),
            Side::Top => (fx * rw, rh - 2.0),
        };
        self.active = true;
        self.rx = rx;
        self.ry = ry;
        let _ = self.out.send(Msg::MoveTo { x: rx, y: ry });
        log!("[server] control -> {}", self.remote_name);
        true
    }

    /// Called with relative mouse motion while active.
    /// Returns Some(local position) when control returns to this machine.
    pub fn motion(&mut self, dx: f64, dy: f64) -> Option<(f64, f64)> {
        let Some((rw, rh)) = self.remote else {
            self.active = false;
            return Some(self.center());
        };
        let (w, h) = self.local;
        self.rx += dx;
        self.ry += dy;
        let leave = match self.side {
            Side::Right => self.rx < 0.0,
            Side::Left => self.rx > rw - 1.0,
            Side::Bottom => self.ry < 0.0,
            Side::Top => self.ry > rh - 1.0,
        };
        if leave {
            let fx = (self.rx / rw).clamp(0.0, 1.0);
            let fy = (self.ry / rh).clamp(0.0, 1.0);
            self.active = false;
            log!("[server] control -> local");
            return Some(match self.side {
                Side::Right => (w - RETURN_MARGIN, fy * h),
                Side::Left => (RETURN_MARGIN, fy * h),
                Side::Bottom => (fx * w, h - RETURN_MARGIN),
                Side::Top => (fx * w, RETURN_MARGIN),
            });
        }
        self.rx = self.rx.clamp(0.0, rw - 1.0);
        self.ry = self.ry.clamp(0.0, rh - 1.0);
        let _ = self.out.send(Msg::MoveTo { x: self.rx, y: self.ry });
        None
    }

    /// Forward a key, button or wheel event to the client (only while active).
    /// Wheel deltas are in this platform's native units.
    pub fn forward(&mut self, ev: EventType) {
        let swap = self.swap_ctrl_cmd
            && !self.remote_os.is_empty()
            && self.remote_os != std::env::consts::OS;
        let ev = match ev {
            EventType::KeyPress(k) => EventType::KeyPress(map_key(k, swap)),
            EventType::KeyRelease(k) => EventType::KeyRelease(map_key(k, swap)),
            EventType::Wheel { delta_x, delta_y } if cfg!(target_os = "macos") => {
                // Protocol unit is a wheel notch; macOS gives pixels, so accumulate.
                self.wheel_acc.0 += delta_x;
                self.wheel_acc.1 += delta_y;
                let nx = self.wheel_acc.0 / MAC_PIXELS_PER_NOTCH;
                let ny = self.wheel_acc.1 / MAC_PIXELS_PER_NOTCH;
                self.wheel_acc.0 -= nx * MAC_PIXELS_PER_NOTCH;
                self.wheel_acc.1 -= ny * MAC_PIXELS_PER_NOTCH;
                if nx == 0 && ny == 0 {
                    return;
                }
                EventType::Wheel {
                    delta_x: nx,
                    delta_y: ny,
                }
            }
            other => other,
        };
        let _ = self.out.send(Msg::Input(ev));
    }

    pub fn center(&self) -> (f64, f64) {
        ((self.local.0 / 2.0).floor(), (self.local.1 / 2.0).floor())
    }
}

/// So that Ctrl+C / Ctrl+V on Windows becomes Cmd+C / Cmd+V on the Mac (and back).
fn map_key(k: Key, swap: bool) -> Key {
    if !swap {
        return k;
    }
    match k {
        Key::ControlLeft => Key::MetaLeft,
        Key::MetaLeft => Key::ControlLeft,
        Key::ControlRight => Key::MetaRight,
        Key::MetaRight => Key::ControlRight,
        other => other,
    }
}

fn waiting_text(port: u16) -> String {
    match local_ip() {
        Some(ip) => format!("Waiting for client on {ip}:{port}"),
        None => format!("Waiting for client on port {port}"),
    }
}

pub fn start(cfg: &Config) -> Result<(), String> {
    let local = screen_size()?;
    let port = cfg.port;
    log!(
        "[server] screen {}x{}, client screen is on the {:?}",
        local.0,
        local.1,
        cfg.remote_side
    );

    let listen = format!("0.0.0.0:{port}");
    // Retry briefly: after a restart the previous instance may still hold the port.
    let mut attempt = 0;
    let listener = loop {
        match TcpListener::bind(&listen) {
            Ok(l) => break l,
            Err(_) if attempt < 10 => {
                attempt += 1;
                thread::sleep(std::time::Duration::from_millis(300));
            }
            Err(e) => {
                let msg = format!("Cannot listen on port {port}: {e} (is winmac already running?)");
                status::set(State::Error, msg.clone());
                return Err(msg);
            }
        }
    };

    let link = Arc::new(Link::default());
    let (out_tx, out_rx) = channel::<Msg>();
    net::spawn_writer(link.clone(), out_rx);
    net::spawn_pinger(link.clone());
    let clip_in = clipboard::spawn(out_tx.clone());

    let core: SharedCore = Arc::new(Mutex::new(Core {
        local,
        side: cfg.remote_side,
        swap_ctrl_cmd: cfg.swap_ctrl_cmd,
        remote: None,
        remote_os: String::new(),
        remote_name: String::new(),
        conn_gen: 0,
        active: false,
        rx: 0.0,
        ry: 0.0,
        wheel_acc: (0, 0),
        out: out_tx,
    }));

    // Input capture first, so a permission problem is reported right away.
    capture::start(core.clone())?;
    status::set(State::Waiting, waiting_text(port));

    // Network thread: accept clients. A new client replaces the old one.
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let peer = stream
                .peer_addr()
                .map(|a| a.ip().to_string())
                .unwrap_or_default();
            let _ = stream.set_nodelay(true);
            let Ok(read_half) = stream.try_clone() else { continue };

            let my_gen = {
                let mut c = core.lock().unwrap();
                if c.active {
                    c.active = false;
                    capture::release(c.center());
                }
                c.conn_gen += 1;
                c.remote = None;
                c.conn_gen
            };
            link.set(Some(stream));
            link.send(&Msg::Welcome {
                os: std::env::consts::OS.to_string(),
            });
            log!("[server] client connected: {peer}");

            let core = core.clone();
            let link = link.clone();
            let clip_in = clip_in.clone();
            thread::spawn(move || {
                net::read_loop(read_half, |msg| match msg {
                    Msg::Hello { width, height, os } => {
                        log!("[server] client {os}, screen {width}x{height}");
                        let mut c = core.lock().unwrap();
                        if c.conn_gen == my_gen {
                            c.remote = Some((width, height));
                            c.remote_name = format!("{peer} ({os})");
                            c.remote_os = os;
                            status::set(State::Connected, format!("Connected: {}", c.remote_name));
                        }
                    }
                    Msg::Clipboard(text) => {
                        let _ = clip_in.send(text);
                    }
                    _ => {}
                });
                let mut c = core.lock().unwrap();
                if c.conn_gen == my_gen {
                    log!("[server] client disconnected: {peer}");
                    c.remote = None;
                    if c.active {
                        c.active = false;
                        capture::release(c.center());
                    }
                    link.set(None);
                    status::set(State::Waiting, waiting_text(port));
                }
            });
        }
    });
    Ok(())
}
