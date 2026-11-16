//! TCP connection shared between threads.

use crate::protocol::Msg;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// The current connection (if any) to the other machine.
#[derive(Default)]
pub struct Link {
    stream: Mutex<Option<TcpStream>>,
}

impl Link {
    pub fn set(&self, s: Option<TcpStream>) {
        let mut g = self.stream.lock().unwrap();
        if let Some(old) = g.take() {
            let _ = old.shutdown(std::net::Shutdown::Both);
        }
        *g = s;
    }

    /// Send one message. Returns false when not connected or the write failed.
    pub fn send(&self, msg: &Msg) -> bool {
        let mut g = self.stream.lock().unwrap();
        let ok = match g.as_mut() {
            Some(s) => s.write_all(&msg.encode()).is_ok(),
            None => false,
        };
        if !ok {
            if let Some(old) = g.take() {
                let _ = old.shutdown(std::net::Shutdown::Both);
            }
        }
        ok
    }
}

/// Thread that sends every message from the channel to the current connection.
/// (Input hook callbacks must never wait on the network, so they only push to the channel.)
pub fn spawn_writer(link: Arc<Link>, rx: Receiver<Msg>) {
    thread::spawn(move || {
        for msg in rx {
            link.send(&msg);
        }
    });
}

/// No message (not even a Ping) for this long means the connection is dead.
pub const READ_TIMEOUT: Duration = Duration::from_secs(7);

/// Send a Ping every 2 s on the current connection (if any).
pub fn spawn_pinger(link: Arc<Link>) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));
        link.send(&Msg::Ping);
    });
}

/// Read messages line by line until the connection closes or goes silent.
pub fn read_loop(stream: TcpStream, mut on_msg: impl FnMut(Msg)) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                log!("[net] connection lost: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        match Msg::decode(&line) {
            Some(m) => on_msg(m),
            None => log!("[net] ignored invalid message"),
        }
    }
}
