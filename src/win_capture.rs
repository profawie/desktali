//! Input capture on Windows (low-level hooks through `rdev::grab`).
//!
//! While controlling the client, real mouse moves are swallowed, so the local cursor
//! stays parked at an anchor point (centre of the monitor where control left). Each
//! hook event reports where the cursor *would* go; the difference from the anchor is
//! the relative motion sent to the client.

use crate::server::SharedCore;
use crate::status::{self, State};
use rdev::{Event, EventType};
use std::thread;

/// Put the local cursor at a desktop position. SetCursorPos works in desktop
/// coordinates (including monitors left of / above the primary one), does not go
/// through the input hook, and is safe to call from inside the hook callback.
fn set_cursor(pos: (f64, f64)) {
    unsafe {
        winapi::um::winuser::SetCursorPos(pos.0.round() as i32, pos.1.round() as i32);
    }
}

/// Control was taken back without a mouse move (e.g. client disconnected).
pub fn release(pos: (f64, f64)) {
    set_cursor(pos);
}

pub fn start(core: SharedCore) -> Result<(), String> {
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
        if !c.active {
            if c.try_enter(x, y) {
                set_cursor(c.center());
                return None;
            }
            return Some(event);
        }
        let (ax, ay) = c.center();
        let (dx, dy) = (x - ax, y - ay);
        if dx == 0.0 && dy == 0.0 {
            return None;
        }
        if let Some(back) = c.motion(dx, dy) {
            set_cursor(back);
        } else {
            // Keep the cursor parked (normally it already is, as moves are swallowed).
            set_cursor((ax, ay));
        }
        return None;
    }

    if !c.active {
        return Some(event);
    }
    c.forward(event.event_type);
    None
}
