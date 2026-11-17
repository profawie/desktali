//! Client: the controlled machine. It receives mouse movement and keys from the
//! server, replays them locally, and syncs the clipboard.

use crate::config::Config;
use crate::net::{self, Link};
use crate::protocol::Msg;
use crate::status::{self, State};
use crate::{clipboard, files};
use crate::screen;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// What the client is doing right now; turned into the tray status by `refresh_status`.
#[derive(Clone, PartialEq)]
enum Conn {
    Connecting(String),
    /// TCP connected, waiting for the server's Welcome.
    Handshake(String),
    Connected(String),
    Failed(String),
}

static CONN: Mutex<Option<Conn>> = Mutex::new(None);

fn set_conn(c: Conn) {
    *CONN.lock().unwrap() = Some(c);
    refresh_status();
}

/// Combine the connection state with the macOS permission check.
fn refresh_status() {
    let Some(conn) = CONN.lock().unwrap().clone() else { return };
    #[cfg(target_os = "macos")]
    let allowed = crate::mac_inject::accessibility_trusted(false);
    #[cfg(not(target_os = "macos"))]
    let allowed = true;

    let (state, text) = match conn {
        Conn::Connecting(t) | Conn::Handshake(t) => (State::Waiting, t),
        Conn::Failed(t) => (State::Error, t),
        Conn::Connected(t) => (State::Connected, t),
    };
    let (state, text) = if allowed {
        (state, text)
    } else {
        (
            State::Error,
            format!("{text}. Accessibility permission missing: allow rtetherfy in System Settings > Privacy & Security > Accessibility"),
        )
    };
    if status::get() != (state, text.clone()) {
        status::set(state, text);
    }
}

pub fn start(cfg: &Config) {
    if cfg.server_address.trim().is_empty() {
        status::set(State::Error, "Set the server address (menu: Server address...)");
        return;
    }
    let addr = cfg.server_socket_addr();

    #[cfg(target_os = "macos")]
    {
        // Show macOS's permission prompt once, then keep the status up to date
        // (the permission can be granted while we run).
        crate::mac_inject::accessibility_trusted(true);
        thread::spawn(|| loop {
            thread::sleep(Duration::from_secs(2));
            refresh_status();
        });
    }

    thread::spawn(move || run(&addr));
}

fn run(addr: &str) {
    let desk = match screen::desktop() {
        Ok(s) => s,
        Err(e) => {
            status::set(State::Error, e);
            return;
        }
    };
    let (w, h) = (desk.w, desk.h);
    log!("[client] desktop {w}x{h} at ({}, {})", desk.x, desk.y);

    let link = Arc::new(Link::default());
    let (out_tx, out_rx) = channel::<Msg>();
    net::spawn_writer(link.clone(), out_rx);
    net::spawn_pinger(link.clone());
    files::attach(link.clone());
    let clip_in = clipboard::spawn(out_tx.clone());

    #[cfg(target_os = "macos")]
    let mut inj = crate::mac_inject::Injector::new();
    #[cfg(target_os = "macos")]
    let mut sim = move |ev: &rdev::EventType| inj.inject(ev);
    #[cfg(not(target_os = "macos"))]
    let sim = |ev: &rdev::EventType| match *ev {
        // Client coordinates start at the desktop's top-left corner, which on
        // Windows can be negative (monitor left of / above the primary one).
        #[cfg(windows)]
        rdev::EventType::MouseMove { x, y } => screen::inject_move(x + desk.x, y + desk.y),
        _ => sim_rdev(ev),
    };

    loop {
        set_conn(Conn::Connecting(format!("Connecting to {addr}...")));
        match connect(addr) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                let read_half = match stream.try_clone() {
                    Ok(s) => s,
                    Err(e) => {
                        log!("[client] {e}");
                        continue;
                    }
                };
                link.set(Some(stream));
                link.send(&Msg::Hello {
                    width: w,
                    height: h,
                    os: std::env::consts::OS.to_string(),
                });
                log!("[client] TCP connected to {addr}, waiting for server");
                set_conn(Conn::Handshake(format!("Connected to {addr}, waiting for server...")));

                let mut welcomed = false;
                let mut last_move = Instant::now() - Duration::from_secs(60);
                net::read_loop(read_half, |msg| match msg {
                    Msg::Welcome { os } => {
                        welcomed = true;
                        log!("[client] server is {os}");
                        files::set_peer(Some((addr.to_string(), os.clone())));
                        set_conn(Conn::Connected(format!("Connected to {addr} ({os})")));
                    }
                    Msg::MoveTo { x, y } => {
                        if last_move.elapsed() > Duration::from_secs(1) {
                            log!("[client] receiving mouse at {x:.0},{y:.0}");
                        }
                        last_move = Instant::now();
                        sim(&rdev::EventType::MouseMove { x, y })
                    }
                    Msg::Input(ev) => sim(&ev),
                    m @ (Msg::Clipboard(_) | Msg::Image { .. } | Msg::Files(_)) => {
                        let _ = clip_in.send(m);
                    }
                    Msg::FsReq { id, op } => files::on_request(id, op, link.clone(), addr.to_string()),
                    Msg::FsResp { id, result } => files::on_reply(id, result),
                    Msg::Hello { .. } | Msg::Ping => {}
                });
                link.set(None);
                files::set_peer(None);

                if welcomed {
                    log!("[client] disconnected from {addr}");
                } else {
                    let msg = format!(
                        "{addr} accepted the connection but is not a rtetherfy server (wrong IP? another app on this port?)"
                    );
                    log!("[client] {msg}");
                    set_conn(Conn::Failed(msg));
                    thread::sleep(Duration::from_secs(5));
                }
            }
            Err(e) => {
                log!("[client] cannot reach {addr}: {e}");
                set_conn(Conn::Connecting(format!("Cannot reach {addr} ({e}), retrying...")));
            }
        }
        thread::sleep(Duration::from_secs(2));
    }
}

fn connect(addr: &str) -> std::io::Result<TcpStream> {
    let sa = addr
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::other("invalid address"))?;
    TcpStream::connect_timeout(&sa, Duration::from_secs(3))
}

#[cfg(not(target_os = "macos"))]
fn sim_rdev(ev: &rdev::EventType) {
    if let Err(e) = rdev::simulate(ev) {
        log!("[client] simulate {ev:?} failed: {e:?}");
    }
}
