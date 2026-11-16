//! Server: mesin yang ada mouse fizikal.
//!
//! Ia memasang hook input global (rdev::grab). Bila kursor sampai ke tepi skrin
//! yang dikonfigurasi, kawalan "menyeberang" ke client: semua input tempatan disekat
//! dan dihantar ke client. Bila kursor maya di client keluar semula melalui tepi
//! yang bertentangan, kawalan kembali ke mesin ini.

use crate::net::{self, Link};
use crate::protocol::Msg;
use crate::{clipboard, screen_size, Side};
use rdev::{Event, EventType, Key};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

/// Berapa piksel dari tepi dikira "sampai ke tepi".
const EDGE: f64 = 2.0;
/// Bila kembali, letak kursor sejauh ini dari tepi supaya tidak terus menyeberang semula.
const RETURN_MARGIN: f64 = 6.0;
/// Berapa piksel scroll macOS dikira satu "notch" roda mouse.
const MAC_PIXELS_PER_NOTCH: i64 = 10;

struct State {
    local: (f64, f64),
    side: Side,
    swap_ctrl_cmd: bool,
    /// Saiz skrin client, jika ada client bersambung.
    remote: Option<(f64, f64)>,
    remote_os: String,
    /// Nombor sambungan semasa (untuk abaikan sambungan lama yang putus).
    conn_gen: u64,
    /// True bila mouse/keyboard sedang mengawal client.
    active: bool,
    /// Menunggu kursor tempatan sampai ke titik anchor selepas warp.
    anchor_pending: bool,
    /// Kedudukan kursor maya di skrin client.
    rx: f64,
    ry: f64,
    /// Baki scroll trackpad/mouse macOS (unit piksel) yang belum jadi satu "notch".
    wheel_acc: (i64, i64),
}

impl State {
    fn anchor(&self) -> (f64, f64) {
        ((self.local.0 / 2.0).floor(), (self.local.1 / 2.0).floor())
    }
}

pub fn run(listen: &str, side: Side, swap_ctrl_cmd: bool) {
    let local = screen_size();
    println!(
        "[server] saiz skrin {}x{}, skrin client di sebelah {:?}",
        local.0, local.1, side
    );

    let state = Arc::new(Mutex::new(State {
        local,
        side,
        swap_ctrl_cmd,
        remote: None,
        remote_os: String::new(),
        conn_gen: 0,
        active: false,
        anchor_pending: false,
        rx: 0.0,
        ry: 0.0,
        wheel_acc: (0, 0),
    }));

    let link = Arc::new(Link::default());
    let (out_tx, out_rx) = channel::<Msg>();
    net::spawn_writer(link.clone(), out_rx);
    let clip_in = clipboard::spawn(out_tx.clone());

    // Thread yang menggerakkan kursor tempatan (warp). Tidak boleh dibuat terus
    // dalam callback hook kerana boleh deadlock pada Windows.
    let (warp_tx, warp_rx) = channel::<(f64, f64)>();
    thread::spawn(move || {
        for (x, y) in warp_rx {
            let _ = rdev::simulate(&EventType::MouseMove { x, y });
        }
    });

    // Thread rangkaian: terima sambungan client.
    let listener = TcpListener::bind(listen).unwrap_or_else(|e| {
        eprintln!("[server] gagal listen di {listen}: {e}");
        std::process::exit(1);
    });
    println!("[server] menunggu client di {listen}");
    {
        let state = state.clone();
        let link = link.clone();
        let warp_tx = warp_tx.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let peer = stream
                    .peer_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_default();
                let _ = stream.set_nodelay(true);
                let Ok(read_half) = stream.try_clone() else {
                    continue;
                };

                let my_gen = {
                    let mut s = state.lock().unwrap();
                    s.conn_gen += 1;
                    s.remote = None;
                    s.active = false;
                    s.conn_gen
                };
                link.set(Some(stream));
                println!("[server] client bersambung: {peer}");

                let state = state.clone();
                let link = link.clone();
                let warp_tx = warp_tx.clone();
                let clip_in = clip_in.clone();
                thread::spawn(move || {
                    net::read_loop(read_half, |msg| match msg {
                        Msg::Hello { width, height, os } => {
                            println!("[server] client {os}, skrin {width}x{height}");
                            let mut s = state.lock().unwrap();
                            if s.conn_gen == my_gen {
                                s.remote = Some((width, height));
                                s.remote_os = os;
                            }
                        }
                        Msg::Clipboard(text) => {
                            let _ = clip_in.send(text);
                        }
                        _ => {}
                    });
                    let mut s = state.lock().unwrap();
                    if s.conn_gen == my_gen {
                        println!("[server] client terputus: {peer}");
                        s.remote = None;
                        if s.active {
                            s.active = false;
                            let _ = warp_tx.send(s.anchor());
                        }
                        link.set(None);
                    }
                });
            }
        });
    }

    // Hook input global. Mesti berjalan di main thread (keperluan macOS).
    let cb_state = state.clone();
    let result = rdev::grab(move |event: Event| on_event(&cb_state, &out_tx, &warp_tx, event));
    if let Err(e) = result {
        eprintln!("[server] gagal pasang hook input: {e:?}");
        eprintln!("[server] macOS: beri kebenaran Accessibility & Input Monitoring kepada Terminal/winmac.");
        std::process::exit(1);
    }
}

fn on_event(
    state: &Mutex<State>,
    out: &Sender<Msg>,
    warp: &Sender<(f64, f64)>,
    event: Event,
) -> Option<Event> {
    let mut s = state.lock().unwrap();

    if let EventType::MouseMove { x, y } = event.event_type {
        let (w, h) = s.local;
        let (ax, ay) = s.anchor();

        if !s.active {
            let Some((rw, rh)) = s.remote else {
                return Some(event);
            };
            let hit = match s.side {
                Side::Right => x >= w - EDGE,
                Side::Left => x <= EDGE - 1.0,
                Side::Bottom => y >= h - EDGE,
                Side::Top => y <= EDGE - 1.0,
            };
            if !hit {
                return Some(event);
            }
            // Menyeberang ke client.
            let fy = (y / h).clamp(0.0, 1.0);
            let fx = (x / w).clamp(0.0, 1.0);
            let (rx, ry) = match s.side {
                Side::Right => (1.0, fy * rh),
                Side::Left => (rw - 2.0, fy * rh),
                Side::Bottom => (fx * rw, 1.0),
                Side::Top => (fx * rw, rh - 2.0),
            };
            s.active = true;
            s.anchor_pending = true;
            s.rx = rx;
            s.ry = ry;
            let _ = out.send(Msg::MoveTo { x: rx, y: ry });
            let _ = warp.send((ax, ay));
            println!("[server] kawalan -> client");
            return None;
        }

        // Sedang mengawal client.
        // (Toleransi 1.5px kerana SendInput di Windows boleh tersasar 1 piksel.)
        if (x - ax).abs() < 1.5 && (y - ay).abs() < 1.5 {
            // Ini event warp kita sendiri: biarkan kursor tempatan pergi ke anchor.
            s.anchor_pending = false;
            return Some(event);
        }
        if s.anchor_pending {
            return None;
        }
        let Some((rw, rh)) = s.remote else {
            s.active = false;
            return Some(event);
        };
        s.rx += x - ax;
        s.ry += y - ay;

        let leave = match s.side {
            Side::Right => s.rx < 0.0,
            Side::Left => s.rx > rw - 1.0,
            Side::Bottom => s.ry < 0.0,
            Side::Top => s.ry > rh - 1.0,
        };
        if leave {
            let fx = (s.rx / rw).clamp(0.0, 1.0);
            let fy = (s.ry / rh).clamp(0.0, 1.0);
            let back = match s.side {
                Side::Right => (w - RETURN_MARGIN, fy * h),
                Side::Left => (RETURN_MARGIN, fy * h),
                Side::Bottom => (fx * w, h - RETURN_MARGIN),
                Side::Top => (fx * w, RETURN_MARGIN),
            };
            s.active = false;
            let _ = warp.send(back);
            println!("[server] kawalan -> tempatan");
            return None;
        }

        s.rx = s.rx.clamp(0.0, rw - 1.0);
        s.ry = s.ry.clamp(0.0, rh - 1.0);
        let _ = out.send(Msg::MoveTo { x: s.rx, y: s.ry });
        // Kembalikan kursor tempatan ke anchor (perlu pada macOS; tidak berbahaya pada Windows).
        let _ = warp.send((ax, ay));
        return None;
    }

    if !s.active {
        return Some(event);
    }

    // Kekunci, butang dan scroll: hantar ke client, sekat secara tempatan.
    let swap = s.swap_ctrl_cmd && !s.remote_os.is_empty() && s.remote_os != std::env::consts::OS;
    let ev = match event.event_type {
        EventType::KeyPress(k) => EventType::KeyPress(map_key(k, swap)),
        EventType::KeyRelease(k) => EventType::KeyRelease(map_key(k, swap)),
        EventType::Wheel { delta_x, delta_y } => {
            // Protokol: scroll dalam unit "notch" roda mouse (seperti Windows).
            if cfg!(target_os = "macos") {
                // macOS memberi delta dalam piksel; kumpul sehingga cukup satu notch.
                s.wheel_acc.0 += delta_x;
                s.wheel_acc.1 += delta_y;
                let nx = s.wheel_acc.0 / MAC_PIXELS_PER_NOTCH;
                let ny = s.wheel_acc.1 / MAC_PIXELS_PER_NOTCH;
                s.wheel_acc.0 -= nx * MAC_PIXELS_PER_NOTCH;
                s.wheel_acc.1 -= ny * MAC_PIXELS_PER_NOTCH;
                if nx == 0 && ny == 0 {
                    return None;
                }
                EventType::Wheel {
                    delta_x: nx,
                    delta_y: ny,
                }
            } else {
                EventType::Wheel { delta_x, delta_y }
            }
        }
        other => other,
    };
    let _ = out.send(Msg::Input(ev));
    None
}

/// Supaya Ctrl+C / Ctrl+V di Windows menjadi Cmd+C / Cmd+V di Mac (dan sebaliknya).
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
