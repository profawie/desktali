mod client;
mod clipboard;
#[cfg(target_os = "macos")]
mod mac_inject;
mod net;
mod protocol;
mod server;

use clap::{Parser, Subcommand, ValueEnum};

pub const DEFAULT_PORT: u16 = 24800;

#[derive(Parser)]
#[command(
    name = "winmac",
    version,
    about = "Kongsi satu mouse/keyboard dan clipboard antara Windows dan macOS melalui LAN"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Jalankan pada mesin yang ada mouse fizikal.
    Server {
        /// Di sebelah mana skrin mesin satu lagi (relatif kepada skrin ini).
        #[arg(long, value_enum, default_value_t = Side::Right)]
        remote_side: Side,
        /// Alamat untuk listen.
        #[arg(long, default_value_t = format!("0.0.0.0:{DEFAULT_PORT}"))]
        listen: String,
        /// Jangan tukar Ctrl <-> Cmd/Win apabila OS kedua-dua mesin berbeza.
        #[arg(long)]
        no_swap_ctrl_cmd: bool,
    },
    /// Jalankan pada mesin yang dikawal (tiada mouse).
    Client {
        /// IP server, contoh 192.168.1.10 atau 192.168.1.10:24800
        server: String,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

fn main() {
    #[cfg(windows)]
    make_dpi_aware();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Server {
            remote_side,
            listen,
            no_swap_ctrl_cmd,
        } => server::run(&listen, remote_side, !no_swap_ctrl_cmd),
        Cmd::Client { server } => {
            let addr = if server.contains(':') {
                server
            } else {
                format!("{server}:{DEFAULT_PORT}")
            };
            client::run(&addr)
        }
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
