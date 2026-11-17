//! System tray (Windows) / menu bar (macOS) app: shows the connection status and
//! lets the user change the configuration. Changing a setting saves the config and
//! restarts the app.

use crate::config::{self, Config, Mode, Side};
use crate::status::{self, State};
use std::cell::RefCell;
use std::process::Command;
use std::thread;
use tray_icon::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

struct Ui {
    cfg: Config,
    tray: TrayIcon,
    status_item: MenuItem,
    shown: Option<(State, String)>,
    mode_server: MenuId,
    mode_client: MenuId,
    sides: Vec<(MenuId, Side)>,
    server_addr: MenuId,
    open_config: MenuId,
    open_log: MenuId,
    restart: MenuId,
    quit: MenuId,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    /// Config for the UI, built on the first tick (macOS wants the run loop running first).
    static PENDING: RefCell<Option<Config>> = const { RefCell::new(None) };
}

pub fn run() -> ! {
    let cfg = Config::load();
    log!("[tray] starting, config {}", config::path().display());
    if let Err(e) = crate::start_engine(&cfg) {
        log!("[tray] engine not started: {e}");
    }
    platform_loop(cfg)
}

fn build(cfg: Config) -> Ui {
    let is_server = cfg.mode == Mode::Server;

    let status_item = MenuItem::new("Starting...", false, None);
    let mode_server = CheckMenuItem::new("Server (this computer has the mouse)", true, is_server, None);
    let mode_client = CheckMenuItem::new("Client (controlled by the other computer)", true, !is_server, None);
    let mode = Submenu::with_items("Mode", true, &[&mode_server, &mode_client]).unwrap();

    let side_items: Vec<(CheckMenuItem, Side)> = Side::ALL
        .iter()
        .map(|&s| (CheckMenuItem::new(s.label(), is_server, cfg.remote_side == s, None), s))
        .collect();
    let side_refs: Vec<&dyn tray_icon::menu::IsMenuItem> = side_items
        .iter()
        .map(|(i, _)| i as &dyn tray_icon::menu::IsMenuItem)
        .collect();
    let side = Submenu::with_items("Other screen is on the", is_server, &side_refs).unwrap();

    let addr_label = if cfg.server_address.trim().is_empty() {
        "Server address: (not set)...".to_string()
    } else {
        format!("Server address: {}...", cfg.server_address.trim())
    };
    let server_addr = MenuItem::new(addr_label, !is_server, None);
    let open_config = MenuItem::new("Open config file", true, None);
    let open_log = MenuItem::new("Open log file", true, None);
    let restart = MenuItem::new("Restart", true, None);
    let quit = MenuItem::new("Quit rtetherfy", true, None);

    let menu = Menu::new();
    let _ = menu.append_items(&[
        &status_item,
        &PredefinedMenuItem::separator(),
        &mode,
        &side,
        &server_addr,
        &PredefinedMenuItem::separator(),
        &open_config,
        &open_log,
        &restart,
        &PredefinedMenuItem::separator(),
        &quit,
    ]);

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("rtetherfy")
        .with_icon(icon(State::Waiting))
        .build()
        .expect("create tray icon");

    Ui {
        mode_server: mode_server.id().clone(),
        mode_client: mode_client.id().clone(),
        sides: side_items.iter().map(|(i, s)| (i.id().clone(), *s)).collect(),
        server_addr: server_addr.id().clone(),
        open_config: open_config.id().clone(),
        open_log: open_log.id().clone(),
        restart: restart.id().clone(),
        quit: quit.id().clone(),
        cfg,
        tray,
        status_item,
        shown: None,
    }
}

/// Called periodically on the UI thread.
fn tick() {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        if ui.is_none() {
            if let Some(cfg) = PENDING.with(|p| p.borrow_mut().take()) {
                *ui = Some(build(cfg));
            }
        }
        let Some(ui) = ui.as_mut() else { return };

        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            handle(ui, &ev.id);
        }

        let st = status::get();
        if ui.shown.as_ref() != Some(&st) {
            let mode = match ui.cfg.mode {
                Mode::Server => "server",
                Mode::Client => "client",
            };
            let text = if st.1.is_empty() { "Starting...".to_string() } else { st.1.clone() };
            ui.status_item.set_text(&text);
            let _ = ui.tray.set_tooltip(Some(format!("rtetherfy ({mode}): {text}")));
            let _ = ui.tray.set_icon(Some(icon(st.0)));
            ui.shown = Some(st);
        }
    });
}

fn handle(ui: &mut Ui, id: &MenuId) {
    let mut cfg = ui.cfg.clone();
    if *id == ui.mode_server {
        cfg.mode = Mode::Server;
    } else if *id == ui.mode_client {
        cfg.mode = Mode::Client;
    } else if let Some((_, s)) = ui.sides.iter().find(|(i, _)| i == id) {
        cfg.remote_side = *s;
    } else if *id == ui.server_addr {
        let current = cfg.server_address.clone();
        thread::spawn(move || {
            if let Some(v) = ask_text("IP address of the computer that has the mouse (server):", &current) {
                let v = v.trim().to_string();
                if !v.is_empty() && v != current {
                    let mut cfg = Config::load();
                    cfg.server_address = v;
                    save_and_restart(&cfg);
                }
            }
        });
        return;
    } else if *id == ui.open_config {
        let _ = ui.cfg.save();
        open_in_editor(&config::path());
        return;
    } else if *id == ui.open_log {
        open_in_editor(&crate::log::path());
        return;
    } else if *id == ui.restart {
        restart();
    } else if *id == ui.quit {
        quit();
    } else {
        return;
    }
    save_and_restart(&cfg);
}

fn save_and_restart(cfg: &Config) {
    if let Err(e) = cfg.save() {
        log!("[tray] cannot save config: {e}");
    }
    restart();
}

fn restart() -> ! {
    log!("[tray] restarting");
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).spawn();
    }
    quit()
}

fn quit() -> ! {
    #[cfg(target_os = "macos")]
    let _ = core_graphics::display::CGDisplay::associate_mouse_and_mouse_cursor_position(true);
    std::process::exit(0)
}

/// A filled circle: green = connected, amber = waiting, red = needs attention.
fn icon(state: State) -> Icon {
    const N: u32 = 32;
    let (r, g, b) = match state {
        State::Connected => (0x2e, 0xb8, 0x4f),
        State::Waiting => (0xf0, 0xa0, 0x20),
        State::Error => (0xe0, 0x3c, 0x31),
    };
    let mut px = Vec::with_capacity((N * N * 4) as usize);
    let c = (N as f32 - 1.0) / 2.0;
    for y in 0..N {
        for x in 0..N {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let outer = 14.5;
            let a = (outer + 0.5 - d).clamp(0.0, 1.0);
            // White ring between radius 9 and 11 for a "connection" look.
            let ring = d > 8.5 && d < 11.0;
            let (pr, pg, pb) = if ring { (255, 255, 255) } else { (r, g, b) };
            px.extend_from_slice(&[pr, pg, pb, (a * 255.0) as u8]);
        }
    }
    Icon::from_rgba(px, N, N).expect("icon")
}

fn open_in_editor(path: &std::path::Path) {
    #[cfg(windows)]
    let r = Command::new("notepad.exe").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let r = Command::new("open").arg("-t").arg(path).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let r = Command::new("xdg-open").arg(path).spawn();
    if let Err(e) = r {
        log!("[tray] cannot open {}: {e}", path.display());
    }
}

/// Show a small text input dialog. Returns None when cancelled.
fn ask_text(prompt: &str, default: &str) -> Option<String> {
    #[cfg(windows)]
    let out = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let q = |s: &str| s.replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName Microsoft.VisualBasic; \
             [Microsoft.VisualBasic.Interaction]::InputBox('{}', 'rtetherfy', '{}')",
            q(prompt),
            q(default)
        );
        Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };
    #[cfg(target_os = "macos")]
    let out = {
        let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "text returned of (display dialog \"{}\" default answer \"{}\" with title \"rtetherfy\")",
            q(prompt),
            q(default)
        );
        Command::new("osascript").args(["-e", &script]).output()
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let out: std::io::Result<std::process::Output> = Err(std::io::Error::other("unsupported"));

    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            (!s.is_empty()).then_some(s)
        }
        Ok(_) => None,
        Err(e) => {
            log!("[tray] cannot show input dialog: {e}");
            None
        }
    }
}

#[cfg(windows)]
fn platform_loop(cfg: Config) -> ! {
    use std::ptr::null_mut;
    use winapi::um::winuser::{
        DispatchMessageW, GetMessageW, SetTimer, TranslateMessage, MSG, WM_TIMER,
    };
    PENDING.with(|p| *p.borrow_mut() = Some(cfg));
    tick();
    unsafe {
        SetTimer(null_mut(), 0, 300, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.message == WM_TIMER && msg.hwnd.is_null() {
                tick();
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    quit()
}

#[cfg(target_os = "macos")]
fn platform_loop(cfg: Config) -> ! {
    use core_foundation::date::CFDate;
    use core_foundation::runloop::{
        kCFRunLoopCommonModes, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerRef,
    };
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    extern "C" fn on_timer(_t: CFRunLoopTimerRef, _info: *mut std::ffi::c_void) {
        tick();
    }

    let mtm = MainThreadMarker::new().expect("tray must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // Menu bar only, no Dock icon.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // Opt out of App Nap: a menu bar app with no windows would otherwise be
    // throttled and replay the remote mouse late or drop the connection.
    {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
            NSActivityOptions::UserInitiatedAllowingIdleSystemSleep
                | NSActivityOptions::LatencyCritical,
            &NSString::from_str("Sharing mouse and clipboard"),
        );
        std::mem::forget(token);
    }

    PENDING.with(|p| *p.borrow_mut() = Some(cfg));

    let timer = CFRunLoopTimer::new(
        CFDate::now().abs_time() + 0.05,
        0.3,
        0,
        0,
        on_timer,
        std::ptr::null_mut(),
    );
    CFRunLoop::get_main().add_timer(&timer, unsafe { kCFRunLoopCommonModes });
    app.run();
    quit()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn platform_loop(_cfg: Config) -> ! {
    log!("[tray] tray mode is only supported on Windows and macOS; use `rtetherfy server` or `rtetherfy client`");
    std::process::exit(1)
}
