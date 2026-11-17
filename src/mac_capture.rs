//! Input capture on macOS with a CoreGraphics event tap on the main run loop.
//!
//! While controlling the client, the local cursor is detached from the mouse
//! (`CGAssociateMouseAndMouseCursorPosition(false)`) and relative motion is read from
//! the events' delta fields.

use crate::mac_inject::{accessibility_trusted, modifier_flag};
use crate::mac_keys::key_from_code;
use crate::server::SharedCore;
use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    EventField,
};
use core_graphics::geometry::CGPoint;
use rdev::{Button, EventType, Key};
use std::sync::atomic::{AtomicPtr, Ordering};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

static TAP_PORT: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

fn set_cursor_detached(detached: bool) {
    let _ = CGDisplay::associate_mouse_and_mouse_cursor_position(!detached);
}

/// Give control back to this machine and put the cursor at `pos`.
pub fn release(pos: (f64, f64)) {
    let _ = CGDisplay::warp_mouse_cursor_position(CGPoint::new(pos.0, pos.1));
    set_cursor_detached(false);
}

pub fn start(core: SharedCore) -> Result<(), String> {
    if !accessibility_trusted(true) {
        let msg = "Allow Accessibility and Input Monitoring for rtetherfy (System Settings > Privacy & Security), then restart";
        crate::status::set(crate::status::State::Error, msg);
        return Err(msg.into());
    }
    let events = vec![
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDragged,
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
        CGEventType::RightMouseDown,
        CGEventType::RightMouseUp,
        CGEventType::OtherMouseDown,
        CGEventType::OtherMouseUp,
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::FlagsChanged,
        CGEventType::ScrollWheel,
    ];
    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        events,
        move |_proxy, ty, ev| on_event(&core, ty, ev),
    )
    .map_err(|_| {
        let msg = "Cannot capture input: allow Input Monitoring and Accessibility for rtetherfy, then restart";
        crate::status::set(crate::status::State::Error, msg);
        msg.to_string()
    })?;
    TAP_PORT.store(
        tap.mach_port.as_concrete_TypeRef() as *mut _,
        Ordering::SeqCst,
    );
    let source = tap
        .mach_port
        .create_runloop_source(0)
        .map_err(|_| "Cannot create run loop source".to_string())?;
    CFRunLoop::get_main().add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    // Lives for the whole process.
    std::mem::forget(source);
    std::mem::forget(tap);
    Ok(())
}

fn swallow(ev: &CGEvent) -> Option<CGEvent> {
    ev.set_type(CGEventType::Null);
    None
}

fn on_event(core: &SharedCore, ty: CGEventType, ev: &CGEvent) -> Option<CGEvent> {
    // macOS disables slow taps; turn it back on.
    if matches!(
        ty,
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
    ) {
        let port = TAP_PORT.load(Ordering::SeqCst);
        if !port.is_null() {
            unsafe { CGEventTapEnable(port as CFMachPortRef, true) };
        }
        return None;
    }

    let mut c = core.lock().unwrap();
    match ty {
        CGEventType::MouseMoved
        | CGEventType::LeftMouseDragged
        | CGEventType::RightMouseDragged
        | CGEventType::OtherMouseDragged => {
            if !c.active {
                let p = ev.location();
                if c.try_enter(p.x, p.y) {
                    set_cursor_detached(true);
                    return swallow(ev);
                }
                return None;
            }
            let dx = ev.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_X) as f64;
            let dy = ev.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y) as f64;
            if let Some(back) = c.motion(dx, dy) {
                release(back);
            }
            swallow(ev)
        }
        _ if !c.active => None,
        CGEventType::LeftMouseDown => fwd(&mut c, ev, EventType::ButtonPress(Button::Left)),
        CGEventType::LeftMouseUp => fwd(&mut c, ev, EventType::ButtonRelease(Button::Left)),
        CGEventType::RightMouseDown => fwd(&mut c, ev, EventType::ButtonPress(Button::Right)),
        CGEventType::RightMouseUp => fwd(&mut c, ev, EventType::ButtonRelease(Button::Right)),
        CGEventType::OtherMouseDown => fwd(&mut c, ev, EventType::ButtonPress(Button::Middle)),
        CGEventType::OtherMouseUp => fwd(&mut c, ev, EventType::ButtonRelease(Button::Middle)),
        CGEventType::KeyDown => {
            let k = key_of(ev);
            fwd(&mut c, ev, EventType::KeyPress(k))
        }
        CGEventType::KeyUp => {
            let k = key_of(ev);
            fwd(&mut c, ev, EventType::KeyRelease(k))
        }
        CGEventType::FlagsChanged => {
            let k = key_of(ev);
            if k == Key::CapsLock {
                c.forward(EventType::KeyPress(k));
                return fwd(&mut c, ev, EventType::KeyRelease(k));
            }
            let down = modifier_flag(k)
                .map(|f| ev.get_flags().contains(f))
                .unwrap_or(false);
            let e = if down {
                EventType::KeyPress(k)
            } else {
                EventType::KeyRelease(k)
            };
            fwd(&mut c, ev, e)
        }
        CGEventType::ScrollWheel => {
            let dy = ev.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1);
            let dx = ev.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2);
            fwd(
                &mut c,
                ev,
                EventType::Wheel {
                    delta_x: dx,
                    delta_y: dy,
                },
            )
        }
        _ => None,
    }
}

fn key_of(ev: &CGEvent) -> Key {
    key_from_code(ev.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16)
}

fn fwd(c: &mut crate::server::Core, ev: &CGEvent, e: EventType) -> Option<CGEvent> {
    c.forward(e);
    swallow(ev)
}
