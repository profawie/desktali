//! Sambungan TCP yang dikongsi antara thread.

use crate::protocol::Msg;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;

/// Sambungan semasa (jika ada) ke mesin sebelah.
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

    /// Hantar satu mesej. Pulangkan false jika tiada sambungan / gagal.
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

/// Thread yang menghantar semua mesej dari channel ke sambungan semasa.
/// (Callback hook input tidak boleh menunggu rangkaian, jadi ia hanya tolak ke channel.)
pub fn spawn_writer(link: Arc<Link>, rx: Receiver<Msg>) {
    thread::spawn(move || {
        for msg in rx {
            link.send(&msg);
        }
    });
}

/// Baca mesej baris demi baris sehingga sambungan putus.
pub fn read_loop(stream: TcpStream, mut on_msg: impl FnMut(Msg)) {
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        match Msg::decode(&line) {
            Some(m) => on_msg(m),
            None => eprintln!("[net] mesej tidak sah diabaikan"),
        }
    }
}
