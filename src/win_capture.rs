//! Input capture on Windows (low-level hooks through `rdev::grab`).
//!
//! While controlling the client, real mouse moves are swallowed, so the local cursor
//! stays put. Each hook event reports where the cursor *would* go; the difference from
//! an anchor point (screen centre) is the relative motion sent to the client.

use crate::server::SharedCore;
use crate::status::{self, State};
use rdev::{Event, EventType};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use std::thread;

static WARP: OnceLock<Sender<(f64, f64)>> = OnceLock::new();
/// Waiting for the local cursor to arrive at the anchor after a warp.
static ANCHOR_PENDING: AtomicBool = AtomicBool::new(false);

/// Move the local cursor. Done on a separate thread: calling SendInput from inside
/// the hook callback can deadlock.
fn warp(pos: (f64, f64)) {
    if let Some(tx) = WARP.get() {
        let _ = tx.send(pos);
    }
}

/// Control was taken back without a mouse move (e.g. client disconnected).
pub fn release(pos: (f64, f64)) {
    warp(pos);
}

pub fn start(core: SharedCore) -> Result<(), String> {
    let (tx, rx) = channel::<(f64, f64)>();
    let _ = WARP.set(tx);
    thread::spawn(move || {
        for (x, y) in rx {
            let _ = rdev::simulate(&EventType::MouseMove { x, y });
        }
    });

    // The hook runs its own message loop on this thread.
    thread::spawn(move || {
        let result = rdev::grab(move |event: Event| on_event(&core, event));
        if let Err(e) = result {
            status::set(State::Error, format!("Cannot install input hook: {e:?}"));
        }
    });
    Ok(())
}

fn on_event(core: &SharedCore, event: Event) -> Option<Event> {
    let mut c = core.lock().unwrap();

    if let EventType::MouseMove { x, y } = event.event_type {
        let (ax, ay) = c.center();
        if !c.active {
            if c.try_enter(x, y) {
                ANCHOR_PENDING.store(true, Ordering::Relaxed);
                warp((ax, ay));
                return None;
            }
            return Some(event);
        }
        // Our own warp to the anchor (SendInput may land 1px off): let it through.
        if (x - ax).abs() < 1.5 && (y - ay).abs() < 1.5 {
            ANCHOR_PENDING.store(false, Ordering::Relaxed);
            return Some(event);
        }
        if ANCHOR_PENDING.load(Ordering::Relaxed) {
            return None;
        }
        if let Some(back) = c.motion(x - ax, y - ay) {
            warp(back);
        }
        return None;
    }

    if !c.active {
        return Some(event);
    }
    c.forward(event.event_type);
    None
}
