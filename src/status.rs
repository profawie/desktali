//! Connection status shown in the tray icon and menu.

use std::sync::Mutex;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Waiting for / trying to reach the other machine.
    Waiting,
    Connected,
    /// Something needs the user's attention (permission, bad config, port in use...).
    Error,
}

static STATUS: Mutex<(State, String)> = Mutex::new((State::Waiting, String::new()));

pub fn set(state: State, text: impl Into<String>) {
    let text = text.into();
    log!("[status] {text}");
    *STATUS.lock().unwrap() = (state, text);
}

pub fn get() -> (State, String) {
    STATUS.lock().unwrap().clone()
}
