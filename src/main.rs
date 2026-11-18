// No console window when started as a tray app on Windows. In CLI mode we attach to
// the parent terminal's console instead (see `attach_console`).
#![cfg_attr(windows, windows_subsystem = "windows")]

#[macro_use]
mod log;
mod client;
mod clipboard;
mod config;
mod files;
#[cfg(target_os = "macos")]
mod mac_capture;
#[cfg(target_os = "macos")]
mod mac_inject;
#[cfg(target_os = "macos")]
mod mac_keys;
mod net;
mod protocol;
mod screen;
mod server;
mod status;
mod tray;
mod webui;
#[cfg(not(target_os = "macos"))]
mod win_capture;

use config::{Config, Mode, Side};

pub const DEFAULT_PORT: u16 = 24800;

const USAGE: &str = "desktali - share one mouse/keyboard and the clipboard between Windows and macOS

Usage:
  desktali
      Start as a system tray / menu bar app using the saved configuration.

  desktali server [--remote-side left|right|top|bottom] [--port 24800] [--no-swap-ctrl-cmd]
      Run in the terminal on the machine that has the physical mouse.
      --remote-side       where the other machine's screen is (default: right)
      --port              TCP port to listen on (default: 24800)
      --no-swap-ctrl-cmd  do not swap Ctrl <-> Cmd/Win between different OSes

  desktali client <server-ip>[:port]
      Run in the terminal on the controlled machine, e.g. desktali client 192.168.1.10

  desktali --config-path
      Print the location of the configuration file.
";

fn usage_exit(err: &str) -> ! {
    if !err.is_empty() {
        eprintln!("Error: {err}\n");
    }
    eprint!("{USAGE}");
    std::process::exit(if err.is_empty() { 0 } else { 2 });
}

fn main() {
    #[cfg(windows)]
    make_dpi_aware();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        log::init_file();
        tray::run();
    };
    attach_console();
    let rest = &args[1..];

    match cmd.as_str() {
        "server" => {
            let mut cfg = Config {
                mode: Mode::Server,
                ..Config::default()
            };
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                let (key, inline) = match a.split_once('=') {
                    Some((k, v)) => (k, Some(v.to_string())),
                    None => (a.as_str(), None),
                };
                let mut value = || {
                    inline
                        .clone()
                        .or_else(|| it.next().cloned())
                        .unwrap_or_else(|| usage_exit(&format!("{key} needs a value")))
                };
                match key {
                    "--remote-side" => {
                        let v = value();
                        cfg.remote_side = Side::parse(&v)
                            .unwrap_or_else(|| usage_exit(&format!("invalid --remote-side: {v}")))
                    }
                    "--port" => {
                        let v = value();
                        cfg.port = v
                            .parse()
                            .unwrap_or_else(|_| usage_exit(&format!("invalid --port: {v}")))
                    }
                    "--no-swap-ctrl-cmd" => cfg.swap_ctrl_cmd = false,
                    "-h" | "--help" => usage_exit(""),
                    other => usage_exit(&format!("unknown option: {other}")),
                }
            }
            run_cli(cfg)
        }
        "client" => {
            let server = match rest {
                [s] if !s.starts_with('-') => s.clone(),
                _ => usage_exit("client needs exactly one server address"),
            };
            run_cli(Config {
                mode: Mode::Client,
                server_address: server,
                ..Config::default()
            })
        }
        "--config-path" => println!("{}", config::path().display()),
        "-h" | "--help" | "help" => usage_exit(""),
        "-V" | "--version" => println!("desktali {}", env!("CARGO_PKG_VERSION")),
        other => usage_exit(&format!("unknown command: {other}")),
    }
}

/// Run in the terminal (no tray) until killed.
fn run_cli(cfg: Config) {
    if let Err(e) = start_engine(&cfg) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
    // On macOS the input tap lives on the main run loop.
    #[cfg(target_os = "macos")]
    core_foundation::runloop::CFRunLoop::run_current();
    loop {
        std::thread::park();
    }
}

/// Start server or client in background threads according to the configuration.
pub fn start_engine(cfg: &Config) -> Result<(), String> {
    match cfg.mode {
        Mode::Server => server::start(cfg),
        Mode::Client => {
            client::start(cfg);
            Ok(())
        }
    }
}

#[cfg(windows)]
fn attach_console() {
    use winapi::um::wincon::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
    // Start output on a fresh line after the shell prompt.
    println!();
}

#[cfg(not(windows))]
fn attach_console() {}

/// With display scaling (125%/150%) on Windows, hook coordinates and screen size can
/// otherwise be in different units. Make the process DPI aware so all are physical pixels.
#[cfg(windows)]
fn make_dpi_aware() {
    use winapi::shared::windef::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
    use winapi::um::winuser::{SetProcessDPIAware, SetProcessDpiAwarenessContext};
    unsafe {
        // Per-monitor aware: real pixels on every monitor, even with different scaling.
        // (Windows 10 1703+; fall back to system-aware on older versions.)
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
            SetProcessDPIAware();
        }
    }
}

/// Best-effort LAN IP address of this machine (no packets are sent).
pub fn local_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}
