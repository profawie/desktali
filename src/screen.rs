//! Screen geometry in the platform's global cursor coordinates.
//!
//! On Windows this covers every monitor (the "virtual desktop"), so the hand-off
//! edge is the outer edge of the whole desktop, not of the primary monitor.
//! On macOS only the main display is used.

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn center(&self) -> (f64, f64) {
        ((self.x + self.w / 2.0).floor(), (self.y + self.h / 2.0).floor())
    }
}

/// Bounding box of all monitors.
#[cfg(windows)]
pub fn desktop() -> Result<Rect, String> {
    use winapi::um::winuser::*;
    let r = unsafe {
        Rect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN) as f64,
            y: GetSystemMetrics(SM_YVIRTUALSCREEN) as f64,
            w: GetSystemMetrics(SM_CXVIRTUALSCREEN) as f64,
            h: GetSystemMetrics(SM_CYVIRTUALSCREEN) as f64,
        }
    };
    if r.w <= 0.0 || r.h <= 0.0 {
        return Err("cannot read screen size".into());
    }
    Ok(r)
}

/// The monitor containing (or nearest to) a point.
#[cfg(windows)]
pub fn monitor_at(x: f64, y: f64) -> Rect {
    use winapi::shared::windef::POINT;
    use winapi::um::winuser::*;
    unsafe {
        let mon = MonitorFromPoint(
            POINT {
                x: x as i32,
                y: y as i32,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if !mon.is_null() && GetMonitorInfoW(mon, &mut info) != 0 {
            let r = info.rcMonitor;
            return Rect {
                x: r.left as f64,
                y: r.top as f64,
                w: (r.right - r.left) as f64,
                h: (r.bottom - r.top) as f64,
            };
        }
    }
    desktop().unwrap_or(Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    })
}

/// Move the cursor to an absolute desktop position with a real mouse-move input
/// (so apps see hover/drag), handling monitors left of / above the primary one.
#[cfg(windows)]
pub fn inject_move(x: f64, y: f64) {
    use winapi::um::winuser::*;
    let Ok(d) = desktop() else { return };
    let nx = ((x - d.x) * 65535.0 / (d.w - 1.0).max(1.0)).round() as i32;
    let ny = ((y - d.y) * 65535.0 / (d.h - 1.0).max(1.0)).round() as i32;
    unsafe {
        let mut input: INPUT = std::mem::zeroed();
        input.type_ = INPUT_MOUSE;
        let mi = input.u.mi_mut();
        mi.dx = nx;
        mi.dy = ny;
        mi.dwFlags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
        SendInput(1, &mut input, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(not(windows))]
pub fn desktop() -> Result<Rect, String> {
    let (w, h) = rdev::display_size().map_err(|e| format!("cannot read screen size: {e:?}"))?;
    Ok(Rect {
        x: 0.0,
        y: 0.0,
        w: w as f64,
        h: h as f64,
    })
}

#[cfg(not(windows))]
pub fn monitor_at(_x: f64, _y: f64) -> Rect {
    desktop().unwrap_or(Rect {
        x: 0.0,
        y: 0.0,
        w: 1440.0,
        h: 900.0,
    })
}
