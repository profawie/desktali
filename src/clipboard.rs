//! Penyegerakan clipboard (teks) dua hala.
//!
//! Satu thread memiliki objek clipboard. Ia:
//!  * menyemak clipboard tempatan setiap 400ms dan menghantar teks baharu ke mesin sebelah;
//!  * menerima teks dari mesin sebelah (melalui channel) dan menulisnya ke clipboard tempatan.

use crate::protocol::Msg;
use arboard::Clipboard;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(400);
const MAX_LEN: usize = 8 * 1024 * 1024;

/// Mulakan thread clipboard. `out` ialah channel ke penulis rangkaian.
/// Pulangkan Sender untuk teks yang diterima dari mesin sebelah.
pub fn spawn(out: Sender<Msg>) -> Sender<String> {
    let (in_tx, in_rx) = channel::<String>();
    thread::spawn(move || {
        let mut cb = loop {
            match Clipboard::new() {
                Ok(c) => break c,
                Err(e) => {
                    eprintln!("[clipboard] gagal buka clipboard: {e}; cuba lagi...");
                    thread::sleep(Duration::from_secs(2));
                }
            }
        };
        // Jangan hantar apa yang sudah ada dalam clipboard semasa program bermula.
        let mut last: Option<String> = cb.get_text().ok();

        loop {
            match in_rx.recv_timeout(POLL) {
                Ok(text) => {
                    if last.as_deref() != Some(text.as_str()) {
                        match cb.set_text(text.clone()) {
                            Ok(()) => {
                                println!("[clipboard] diterima {} aksara", text.chars().count())
                            }
                            Err(e) => eprintln!("[clipboard] gagal tulis: {e}"),
                        }
                        last = Some(text);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Ok(text) = cb.get_text() {
                        if !text.is_empty()
                            && text.len() <= MAX_LEN
                            && last.as_deref() != Some(text.as_str())
                        {
                            last = Some(text.clone());
                            println!("[clipboard] dihantar {} aksara", text.chars().count());
                            let _ = out.send(Msg::Clipboard(text));
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    in_tx
}
