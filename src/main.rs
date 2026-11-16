mod client;
mod clipboard;
#[cfg(target_os = "macos")]
mod mac_inject;
mod net;
mod protocol;
mod server;

pub const DEFAULT_PORT: u16 = 24800;

const USAGE: &str = "winmac - kongsi satu mouse/keyboard dan clipboard antara Windows dan macOS

Guna:
  winmac server [--remote-side left|right|top|bottom] [--listen 0.0.0.0:24800] [--no-swap-ctrl-cmd]
      Jalankan pada mesin yang ada mouse fizikal.
      --remote-side       di sebelah mana skrin mesin satu lagi (default: right)
      --listen            alamat untuk listen (default: 0.0.0.0:24800)
      --no-swap-ctrl-cmd  jangan tukar Ctrl <-> Cmd/Win antara OS berbeza

  winmac client <IP-server>[:port]
      Jalankan pada mesin yang dikawal, contoh: winmac client 192.168.1.10
";

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

fn usage_exit(err: &str) -> ! {
    if !err.is_empty() {
        eprintln!("Ralat: {err}\n");
    }
    eprint!("{USAGE}");
    std::process::exit(if err.is_empty() { 0 } else { 2 });
}

fn main() {
    #[cfg(windows)]
    make_dpi_aware();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else { usage_exit("") };
    let rest = &args[1..];

    match cmd.as_str() {
        "server" => {
            let mut side = Side::Right;
            let mut listen = format!("0.0.0.0:{DEFAULT_PORT}");
            let mut swap = true;
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
                        .unwrap_or_else(|| usage_exit(&format!("{key} perlukan nilai")))
                };
                match key {
                    "--remote-side" => {
                        side = match value().to_lowercase().as_str() {
                            "left" => Side::Left,
                            "right" => Side::Right,
                            "top" => Side::Top,
                            "bottom" => Side::Bottom,
                            v => usage_exit(&format!("--remote-side tidak sah: {v}")),
                        }
                    }
                    "--listen" => listen = value(),
                    "--no-swap-ctrl-cmd" => swap = false,
                    "-h" | "--help" => usage_exit(""),
                    other => usage_exit(&format!("pilihan tidak dikenali: {other}")),
                }
            }
            server::run(&listen, side, swap)
        }
        "client" => {
            let server = match rest {
                [s] if !s.starts_with('-') => s.clone(),
                _ => usage_exit("client perlukan satu IP server"),
            };
            let addr = if server.contains(':') {
                server
            } else {
                format!("{server}:{DEFAULT_PORT}")
            };
            client::run(&addr)
        }
        "-h" | "--help" | "help" => usage_exit(""),
        "-V" | "--version" => println!("winmac {}", env!("CARGO_PKG_VERSION")),
        other => usage_exit(&format!("arahan tidak dikenali: {other}")),
    }
}

/// Pada Windows dengan display scaling (125%/150%), tanpa ini koordinat hook mouse
/// dan saiz skrin boleh berbeza unit. Jadikan proses DPI-aware supaya semuanya piksel fizikal.
#[cfg(windows)]
fn make_dpi_aware() {
    #[link(name = "user32")]
    extern "system" {
        fn SetProcessDPIAware() -> i32;
    }
    unsafe {
        SetProcessDPIAware();
    }
}

pub fn screen_size() -> (f64, f64) {
    match rdev::display_size() {
        Ok((w, h)) => (w as f64, h as f64),
        Err(e) => {
            eprintln!("Gagal dapatkan saiz skrin: {e:?}");
            std::process::exit(1);
        }
    }
}
