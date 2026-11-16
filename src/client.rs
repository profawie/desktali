//! Client: the controlled machine. It receives mouse movement and keys from the
//! server, replays them locally, and syncs the clipboard.

use crate::config::Config;
use crate::net::{self, Link};
use crate::protocol::Msg;
use crate::status::{self, State};
use crate::{clipboard, screen_size};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn start(cfg: &Config) {
    if cfg.server_address.trim().is_empty() {
        status::set(State::Error, "Set the server address (menu: Server address...)");
        return;
    }
    let addr = cfg.server_socket_addr();
    thread::spawn(move || run(&addr));
}

fn run(addr: &str) {
    let (w, h) = match screen_size() {
        Ok(s) => s,
        Err(e) => {
            status::set(State::Error, e);
            return;
        }
    };
    log!("[client] screen size {w}x{h}");

    #[cfg(target_os = "macos")]
    if !crate::mac_inject::accessibility_trusted(true) {
        status::set(
            State::Error,
            "Allow Accessibility for winmac (System Settings > Privacy & Security), then restart",
        );
        // Keep going: the permission may be granted while we wait for the server.
    }

    let link = Arc::new(Link::default());
    let (out_tx, out_rx) = channel::<Msg>();
    net::spawn_writer(link.clone(), out_rx);
    let clip_in = clipboard::spawn(out_tx.clone());

    #[cfg(target_os = "macos")]
    let mut inj = crate::mac_inject::Injector::new();
    #[cfg(target_os = "macos")]
    let mut sim = move |ev: &rdev::EventType| inj.inject(ev);
    #[cfg(not(target_os = "macos"))]
    let sim = sim_rdev;

    loop {
        if status::get().0 != State::Error {
            status::set(State::Waiting, format!("Connecting to {addr}..."));
        }
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
                status::set(State::Connected, format!("Connected to {addr}"));

                net::read_loop(read_half, |msg| match msg {
                    Msg::MoveTo { x, y } => sim(&rdev::EventType::MouseMove { x, y }),
                    Msg::Input(ev) => sim(&ev),
                    Msg::Clipboard(text) => {
                        let _ = clip_in.send(text);
                    }
                    Msg::Hello { .. } => {}
                });

                link.set(None);
                status::set(State::Waiting, format!("Disconnected from {addr}, retrying..."));
            }
            Err(e) => {
                if status::get().0 != State::Error {
                    status::set(State::Waiting, format!("Cannot reach {addr} ({e}), retrying..."));
                }
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
