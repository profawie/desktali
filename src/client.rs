//! Client: mesin yang dikawal. Ia menerima pergerakan mouse/kekunci dari server
//! dan "menaip" semula secara tempatan, serta menyegerakkan clipboard.

use crate::net::{self, Link};
use crate::protocol::Msg;
use crate::{clipboard, screen_size};
use std::net::TcpStream;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn run(addr: &str) {
    let (w, h) = screen_size();
    println!("[client] saiz skrin {w}x{h}");

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
        println!("[client] menyambung ke {addr} ...");
        match TcpStream::connect(addr) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                println!("[client] bersambung ke {addr}");
                let read_half = match stream.try_clone() {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("[client] {e}");
                        continue;
                    }
                };
                link.set(Some(stream));
                link.send(&Msg::Hello {
                    width: w,
                    height: h,
                    os: std::env::consts::OS.to_string(),
                });

                net::read_loop(read_half, |msg| match msg {
                    Msg::MoveTo { x, y } => sim(&rdev::EventType::MouseMove { x, y }),
                    Msg::Input(ev) => sim(&ev),
                    Msg::Clipboard(text) => {
                        let _ = clip_in.send(text);
                    }
                    Msg::Hello { .. } => {}
                });

                link.set(None);
                println!("[client] sambungan terputus");
            }
            Err(e) => eprintln!("[client] gagal sambung: {e}"),
        }
        thread::sleep(Duration::from_secs(2));
    }
}

#[cfg(not(target_os = "macos"))]
fn sim_rdev(ev: &rdev::EventType) {
    if let Err(e) = rdev::simulate(ev) {
        eprintln!("[client] gagal simulate {ev:?}: {e:?} ");
    }
}
