# winmac

Satu program kecil dalam Rust untuk guna **satu mouse (dan keyboard)** pada dua komputer, Windows dan macOS, melalui rangkaian LAN/WiFi yang sama. Hanya dua fungsi:

1. **Mouse menyeberang skrin.** Gerak mouse melepasi tepi skrin Windows, kursor muncul di MacBook. Gerak balik, ia kembali ke Windows. Klik, scroll dan keyboard ikut ke mesin yang sedang dikawal.
2. **Clipboard dikongsi.** Copy teks di satu mesin, paste di mesin satu lagi (dua hala). Bila mengawal Mac dari keyboard Windows, `Ctrl+C` / `Ctrl+V` ditukar automatik menjadi `Cmd+C` / `Cmd+V`.

## Cara ia berfungsi

Program yang sama (`winmac`) dijalankan di kedua-dua mesin:

* **server**: mesin yang ada mouse fizikal (contoh: laptop Windows).
* **client**: mesin yang dikawal (contoh: MacBook).

Client menyambung ke server melalui TCP port `24800`. Server memasang hook input global; bila kursor sampai ke tepi skrin yang ditetapkan, semua input mouse/keyboard disekat secara tempatan dan dihantar ke client. Teks clipboard disemak setiap 0.4 saat di kedua-dua belah dan dihantar bila berubah.

## Build

Perlu Rust (stable) di kedua-dua mesin.

```sh
git clone https://github.com/profawie/win-mac-desktop.git
cd win-mac-desktop
```

* macOS: `./build.sh`
* Windows (PowerShell): `.\build.ps1`
  (jika PowerShell menyekat skrip: `powershell -ExecutionPolicy Bypass -File .\build.ps1`)

Kedua-dua skrip menjalankan `cargo build --release` dan mencetak lokasi binari.

Di Windows, `build.ps1` menggunakan toolchain Rust **GNU** (`stable-x86_64-pc-windows-gnu`) dan memasangnya sendiri kali pertama. Toolchain ini membawa linker MinGW sendiri, jadi **tidak perlu Visual Studio / Build Tools** (ralat `link.exe not found`). Untuk build manual:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
cargo +stable-x86_64-pc-windows-gnu build --release
```

Hasil binari:

* Windows: `target\release\winmac.exe`
* macOS: `target/release/winmac`

(Setiap push ke GitHub juga membina binari Windows dan macOS dalam tab **Actions**, boleh dimuat turun sebagai artifact.)

## Cara guna (contoh: mouse di Windows, MacBook di sebelah kanan)

### 1. Cari IP laptop Windows

Dalam PowerShell:

```powershell
ipconfig
```

Ambil `IPv4 Address` untuk WiFi/Ethernet, contoh `192.168.1.10`.

### 2. Jalankan server di Windows

```powershell
cd C:\work\personal\win-mac-desktop
.\target\release\winmac.exe server --remote-side right
```

`--remote-side` ialah kedudukan MacBook berbanding skrin Windows: `left`, `right`, `top` atau `bottom`.

Kali pertama, Windows Firewall akan bertanya; benarkan untuk **Private networks**. Jika tidak muncul, buka port secara manual (PowerShell sebagai Administrator):

```powershell
New-NetFirewallRule -DisplayName "winmac" -Direction Inbound -Protocol TCP -LocalPort 24800 -Action Allow -Profile Private
```

### 3. Jalankan client di MacBook

```sh
cd ~/Documents/personal/win-mac-desktop
./target/release/winmac client 192.168.1.10
```

Client akan cuba sambung semula secara automatik jika sambungan putus.

### 4. Kebenaran macOS (wajib)

macOS tidak membenarkan program mengawal mouse/keyboard tanpa kebenaran. Buka
**System Settings → Privacy & Security → Accessibility** dan hidupkan aplikasi yang menjalankan `winmac` (contoh **Terminal** atau **iTerm**). Jika aplikasi itu tiada dalam senarai, tekan `+` dan tambah.

Jika Mac dijadikan **server** (mouse dipasang pada Mac), tambah juga dalam
**Privacy & Security → Input Monitoring**.

Selepas memberi kebenaran, **tutup dan buka semula Terminal**, kemudian jalankan `winmac` sekali lagi.

### Sebaliknya: mouse di Mac, Windows dikawal

```sh
# di Mac (Windows di sebelah kiri)
./target/release/winmac server --remote-side left
```

```powershell
# di Windows
.\target\release\winmac.exe client 192.168.1.20
```

## Pilihan

```
winmac server [--remote-side left|right|top|bottom] [--listen 0.0.0.0:24800] [--no-swap-ctrl-cmd]
winmac client <IP-server>[:port]
```

* `--no-swap-ctrl-cmd`: jangan tukar Ctrl ↔ Cmd/Win apabila mengawal OS yang berbeza.

## Had (versi pertama)

* Hanya **teks** dalam clipboard (bukan gambar atau fail).
* Andaian satu monitor setiap mesin (monitor utama).
* Tiada enkripsi: guna hanya dalam rangkaian rumah/pejabat yang dipercayai.
* Bila Mac menjadi server, menyeret (drag) dengan butang ditekan belum dihantar ke Windows. Disyorkan mouse dipasang pada Windows (Windows sebagai server).
* Semasa mengawal mesin sebelah, kursor mesin server diletak di tengah skrinnya.

## Masalah biasa

| Masalah | Penyelesaian |
| --- | --- |
| Client `gagal sambung` | Semak IP, kedua-dua mesin dalam rangkaian sama, firewall Windows membenarkan port 24800. |
| Kursor tidak bergerak di Mac | Beri kebenaran Accessibility kepada Terminal, kemudian buka semula Terminal. |
| Server Mac: `gagal pasang hook input` | Beri kebenaran Accessibility **dan** Input Monitoring. |
| Windows: `link.exe not found` | Guna `.\build.ps1` (toolchain GNU), bukan `cargo build` biasa. |
| Windows: `dlltool.exe: program not found` | Jalankan `git pull` (versi baharu tidak perlukan dlltool), kemudian `cargo clean` dan `.\build.ps1` semula. Jika masih berlaku: `winget install BrechtSanders.WinLibs.POSIX.UCRT`, buka semula PowerShell. |
| Kursor tidak menyeberang | Pastikan `--remote-side` betul, dan client sudah bersambung (server cetak `client macos, skrin ...`). |
