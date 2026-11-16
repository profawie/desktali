//! Input injection on macOS using CoreGraphics directly.
//!
//! `rdev::simulate` on macOS falls short here:
//!  * moving the mouse with a button held does not produce "drag" events;
//!  * double-clicks are not recognised because the click state is not set;
//!  * modifiers (Cmd/Shift/...) are not reliably applied to the following keys;
//!  * scrolling is in pixels (far too slow for a Windows mouse wheel).

use crate::mac_keys::code_from_key;
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField,
    ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use rdev::{Button, EventType, Key};
use std::time::{Duration, Instant};

const DOUBLE_CLICK: Duration = Duration::from_millis(500);
/// Lines scrolled on macOS per mouse wheel notch.
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
            log!("[mac] cannot create CGEventSource");
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
        match event {
            Some(e) => e.post(CGEventTapLocation::HID),
            None => log!("[mac] cannot create event for {ev:?}"),
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

pub fn modifier_flag(k: Key) -> Option<CGEventFlags> {
    Some(match k {
        Key::ShiftLeft | Key::ShiftRight => CGEventFlags::CGEventFlagShift,
        Key::ControlLeft | Key::ControlRight => CGEventFlags::CGEventFlagControl,
        Key::Alt | Key::AltGr => CGEventFlags::CGEventFlagAlternate,
        Key::MetaLeft | Key::MetaRight => CGEventFlags::CGEventFlagCommand,
        Key::Function => CGEventFlags::CGEventFlagSecondaryFn,
        _ => return None,
    })
}

/// True when this process may control the computer (Accessibility permission).
/// With `prompt`, macOS shows its "allow accessibility" dialog when not yet allowed.
pub fn accessibility_trusted(prompt: bool) -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::{CFString, CFStringRef};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: CFStringRef;
        fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
    }
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let opts = CFDictionary::from_CFType_pairs(&[(
            key.as_CFType(),
            CFBoolean::from(prompt).as_CFType(),
        )]);
        AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef() as *const _)
    }
}
