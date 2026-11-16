//! Two-way clipboard sync (text).
//!
//! One thread owns the clipboard object. It:
//!  * polls the local clipboard every 400 ms and sends new text to the other machine;
//!  * receives text from the other machine (via a channel) and writes it locally.

use crate::protocol::Msg;
use arboard::Clipboard;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(400);
const MAX_LEN: usize = 8 * 1024 * 1024;

/// Start the clipboard thread. `out` is the channel to the network writer.
/// Returns a Sender for text received from the other machine.
pub fn spawn(out: Sender<Msg>) -> Sender<String> {
    let (in_tx, in_rx) = channel::<String>();
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
        let mut last: Option<String> = cb.get_text().ok();

        loop {
            match in_rx.recv_timeout(POLL) {
                Ok(text) => {
                    if last.as_deref() != Some(text.as_str()) {
                        match cb.set_text(text.clone()) {
                            Ok(()) => log!("[clipboard] received {} chars", text.chars().count()),
                            Err(e) => log!("[clipboard] write failed: {e}"),
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
                            log!("[clipboard] sent {} chars", text.chars().count());
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
