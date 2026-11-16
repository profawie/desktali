//! Suntikan input untuk macOS menggunakan CoreGraphics secara terus.
//!
//! `rdev::simulate` di macOS ada beberapa kekurangan untuk kegunaan ini:
//!  * menggerak mouse sambil butang ditekan tidak menghasilkan event "drag";
//!  * klik dua kali (double-click) tidak dikenali kerana "click state" tidak diset;
//!  * kekunci modifier (Cmd/Shift/...) tidak selalu dikenakan pada kekunci seterusnya;
//!  * scroll dalam unit piksel (terlalu perlahan untuk roda mouse Windows).
//!
//! Jadual kod kekunci diambil daripada rdev (MIT) dan ditambah.

use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode, CGMouseButton, EventField,
    ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use rdev::{Button, EventType, Key};
use std::time::{Duration, Instant};

const DOUBLE_CLICK: Duration = Duration::from_millis(500);
/// Satu "notch" roda mouse = berapa baris di macOS.
const LINES_PER_NOTCH: i32 = 3;

pub struct Injector {
    pos: CGPoint,
    flags: CGEventFlags,
    left: bool,
    right: bool,
    other: bool,
    last_click: Option<(Instant, CGPoint, Button)>,
    click_count: i64,
}

impl Injector {
    pub fn new() -> Self {
        Injector {
            pos: CGPoint::new(0.0, 0.0),
            flags: CGEventFlags::CGEventFlagNull,
            left: false,
            right: false,
            other: false,
            last_click: None,
            click_count: 1,
        }
    }

    pub fn inject(&mut self, ev: &EventType) {
        let Ok(src) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
            eprintln!("[mac] gagal cipta CGEventSource");
            return;
        };
        let event = match *ev {
            EventType::MouseMove { x, y } => {
                self.pos = CGPoint::new(x, y);
                let (ty, btn) = if self.left {
                    (CGEventType::LeftMouseDragged, CGMouseButton::Left)
                } else if self.right {
                    (CGEventType::RightMouseDragged, CGMouseButton::Right)
                } else if self.other {
                    (CGEventType::OtherMouseDragged, CGMouseButton::Center)
                } else {
                    (CGEventType::MouseMoved, CGMouseButton::Left)
                };
                CGEvent::new_mouse_event(src, ty, self.pos, btn).ok()
            }
            EventType::ButtonPress(b) => {
                let now = Instant::now();
                self.click_count = match self.last_click {
                    Some((t, p, lb))
                        if lb == b
                            && now.duration_since(t) < DOUBLE_CLICK
                            && (p.x - self.pos.x).abs() < 5.0
                            && (p.y - self.pos.y).abs() < 5.0 =>
                    {
                        self.click_count + 1
                    }
                    _ => 1,
                };
                self.last_click = Some((now, self.pos, b));
                self.button(src, b, true)
            }
            EventType::ButtonRelease(b) => self.button(src, b, false),
            EventType::KeyPress(k) => self.key(src, k, true),
            EventType::KeyRelease(k) => self.key(src, k, false),
            EventType::Wheel { delta_x, delta_y } => CGEvent::new_scroll_event(
                src,
                ScrollEventUnit::LINE,
                2,
                (delta_y as i32) * LINES_PER_NOTCH,
                (delta_x as i32) * LINES_PER_NOTCH,
                0,
            )
            .ok(),
        };
        if let Some(e) = event {
            e.post(CGEventTapLocation::HID);
        }
    }

    fn button(&mut self, src: CGEventSource, b: Button, down: bool) -> Option<CGEvent> {
        let (ty, btn) = match (b, down) {
            (Button::Left, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left),
            (Button::Left, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left),
            (Button::Right, true) => (CGEventType::RightMouseDown, CGMouseButton::Right),
            (Button::Right, false) => (CGEventType::RightMouseUp, CGMouseButton::Right),
            (_, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center),
            (_, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center),
        };
        match b {
            Button::Left => self.left = down,
            Button::Right => self.right = down,
            _ => self.other = down,
        }
        let e = CGEvent::new_mouse_event(src, ty, self.pos, btn).ok()?;
        e.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, self.click_count);
        e.set_flags(self.flags);
        Some(e)
    }

    fn key(&mut self, src: CGEventSource, k: Key, down: bool) -> Option<CGEvent> {
        if let Some(f) = modifier_flag(k) {
            if down {
                self.flags.insert(f);
            } else {
                self.flags.remove(f);
            }
        }
        if k == Key::CapsLock {
            if down {
                self.flags.toggle(CGEventFlags::CGEventFlagAlphaShift);
            } else {
                return None;
            }
        }
        let code = code_from_key(k)?;
        let e = CGEvent::new_keyboard_event(src, code, down).ok()?;
        e.set_flags(self.flags);
        Some(e)
    }
}

fn modifier_flag(k: Key) -> Option<CGEventFlags> {
    Some(match k {
        Key::ShiftLeft | Key::ShiftRight => CGEventFlags::CGEventFlagShift,
        Key::ControlLeft | Key::ControlRight => CGEventFlags::CGEventFlagControl,
        Key::Alt | Key::AltGr => CGEventFlags::CGEventFlagAlternate,
        Key::MetaLeft | Key::MetaRight => CGEventFlags::CGEventFlagCommand,
        Key::Function => CGEventFlags::CGEventFlagSecondaryFn,
        _ => return None,
    })
}

fn code_from_key(key: Key) -> Option<CGKeyCode> {
    Some(match key {
        Key::Alt => 58,
        Key::AltGr => 61,
        Key::Backspace => 51,
        Key::CapsLock => 57,
        Key::ControlLeft => 59,
        Key::ControlRight => 62,
        Key::DownArrow => 125,
        Key::Escape => 53,
        Key::F1 => 122,
        Key::F2 => 120,
        Key::F3 => 99,
        Key::F4 => 118,
        Key::F5 => 96,
        Key::F6 => 97,
        Key::F7 => 98,
        Key::F8 => 100,
        Key::F9 => 101,
        Key::F10 => 109,
        Key::F11 => 103,
        Key::F12 => 111,
        Key::Function => 63,
        Key::LeftArrow => 123,
        Key::MetaLeft => 55,
        Key::MetaRight => 54,
        Key::Return => 36,
        Key::RightArrow => 124,
        Key::ShiftLeft => 56,
        Key::ShiftRight => 60,
        Key::Space => 49,
        Key::Tab => 48,
        Key::UpArrow => 126,
        Key::Delete => 117,
        Key::Home => 115,
        Key::End => 119,
        Key::PageUp => 116,
        Key::PageDown => 121,
        Key::BackQuote => 50,
        Key::Num1 => 18,
        Key::Num2 => 19,
        Key::Num3 => 20,
        Key::Num4 => 21,
        Key::Num5 => 23,
        Key::Num6 => 22,
        Key::Num7 => 26,
        Key::Num8 => 28,
        Key::Num9 => 25,
        Key::Num0 => 29,
        Key::Minus => 27,
        Key::Equal => 24,
        Key::KeyQ => 12,
        Key::KeyW => 13,
        Key::KeyE => 14,
        Key::KeyR => 15,
        Key::KeyT => 17,
        Key::KeyY => 16,
        Key::KeyU => 32,
        Key::KeyI => 34,
        Key::KeyO => 31,
        Key::KeyP => 35,
        Key::LeftBracket => 33,
        Key::RightBracket => 30,
        Key::KeyA => 0,
        Key::KeyS => 1,
        Key::KeyD => 2,
        Key::KeyF => 3,
        Key::KeyG => 5,
        Key::KeyH => 4,
        Key::KeyJ => 38,
        Key::KeyK => 40,
        Key::KeyL => 37,
        Key::SemiColon => 41,
        Key::Quote => 39,
        Key::BackSlash => 42,
        Key::IntlBackslash => 10,
        Key::KeyZ => 6,
        Key::KeyX => 7,
        Key::KeyC => 8,
        Key::KeyV => 9,
        Key::KeyB => 11,
        Key::KeyN => 45,
        Key::KeyM => 46,
        Key::Comma => 43,
        Key::Dot => 47,
        Key::Slash => 44,
        Key::KpReturn => 76,
        Key::KpMinus => 78,
        Key::KpPlus => 69,
        Key::KpMultiply => 67,
        Key::KpDivide => 75,
        Key::KpDelete => 65,
        Key::Kp0 => 82,
        Key::Kp1 => 83,
        Key::Kp2 => 84,
        Key::Kp3 => 85,
        Key::Kp4 => 86,
        Key::Kp5 => 87,
        Key::Kp6 => 88,
        Key::Kp7 => 89,
        Key::Kp8 => 91,
        Key::Kp9 => 92,
        _ => return None,
    })
}
