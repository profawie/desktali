# WinMac

Satu program kecil dalam Rust untuk guna **satu mouse (dan keyboard)** pada dua komputer, Windows dan macOS, melalui rangkaian LAN/WiFi yang sama. Hanya dua fungsi:

1. **Mouse menyeberang skrin.** Gerak mouse melepasi tepi skrin Windows, kursor muncul di MacBook. Gerak balik, ia kembali ke Windows. Klik, scroll dan keyboard ikut ke mesin yang sedang dikawal.
2. **Clipboard dikongsi.** Copy teks di satu mesin, paste di mesin satu lagi (dua hala). Bila mengawal Mac dari keyboard Windows, `Ctrl+C` / `Ctrl+V` ditukar automatik menjadi `Cmd+C` / `Cmd+V`.

Program berjalan sebagai ikon di **system tray** (Windows, sebelah jam) dan **menu bar** (macOS). Warna ikon menunjukkan status:

* 🟢 hijau: bersambung
* 🟠 oren: menunggu / cuba menyambung
* 🔴 merah: perlu tindakan (kebenaran macOS, alamat server belum diset, port digunakan)

Klik ikon untuk menu: status penuh, **Mode** (Server / Client), **Other screen is on the** (Left / Right / Top / Bottom), **Server address...**, buka fail config / log, Restart dan Quit. Menukar tetapan akan simpan config dan restart program secara automatik.

## Cara ia berfungsi

* **Server**: mesin yang ada mouse fizikal (biasanya laptop Windows).
* **Client**: mesin yang dikawal (biasanya MacBook).

Client menyambung ke server melalui TCP port `24800`. Server memasang hook input global; bila kursor sampai ke tepi skrin yang ditetapkan, semua input mouse/keyboard disekat secara tempatan dan dihantar ke client. Teks clipboard disemak setiap 0.4 saat di kedua-dua belah dan dihantar bila berubah.

## Build

```sh
git clone https://github.com/profawie/win-mac-desktop.git
cd win-mac-desktop
```

* **Windows** (PowerShell): `.\build.ps1`
  (jika PowerShell menyekat skrip: `powershell -ExecutionPolicy Bypass -File .\build.ps1`)
  Hasil: `target\release\winmac.exe`
* **macOS**: `./build.sh`
  Hasil: `target/release/WinMac.app` (app menu bar) dan `target/release/winmac` (CLI)

Di Windows, `build.ps1` menggunakan toolchain Rust **GNU** (`stable-x86_64-pc-windows-gnu`) dan memasangnya sendiri kali pertama. Ia tidak perlukan Visual Studio, MinGW atau `dlltool`.

## Cara guna (mouse di Windows, MacBook di sebelah kanan)

### 1. Windows (server)

1. Double-click `target\release\winmac.exe`. Ikon muncul di system tray (mungkin tersembunyi di bawah anak panah `^`).
2. Mode default di Windows ialah **Server**, skrin lain di sebelah **Right**. Tukar melalui menu jika perlu.
3. Hover ikon: tooltip menunjukkan IP Windows, contoh `Waiting for client on 192.168.1.10:24800`.
4. Kali pertama, Windows Firewall akan bertanya; benarkan untuk **Private networks**. Jika tidak muncul (PowerShell sebagai Administrator):

   ```powershell
   New-NetFirewallRule -DisplayName "winmac" -Direction Inbound -Protocol TCP -LocalPort 24800 -Action Allow -Profile Private
   ```

### 2. MacBook (client)

1. Buka `target/release/WinMac.app` (boleh salin ke `/Applications`). Ikon muncul di menu bar.
2. Mode default di Mac ialah **Client**. Klik ikon → **Server address...** → masukkan IP Windows (contoh `192.168.1.10`).
3. macOS akan minta kebenaran **Accessibility**: buka **System Settings → Privacy & Security → Accessibility**, hidupkan **WinMac**, kemudian klik ikon → **Restart**.

Bila ikon kedua-dua mesin hijau, gerak mouse Windows melepasi tepi kanan skrin.

### Sebaliknya: mouse di Mac

Di Mac pilih **Mode → Server** dan sisi skrin Windows. Di Windows pilih **Mode → Client** dan isi **Server address...** dengan IP Mac. Mac sebagai server perlu kebenaran **Accessibility** dan **Input Monitoring**.

### Mula automatik semasa login

* Windows: tekan `Win+R`, taip `shell:startup`, letak shortcut ke `winmac.exe` di folder itu.
* macOS: **System Settings → General → Login Items** → `+` → pilih `WinMac.app`.

## Konfigurasi

Fail config (JSON), boleh dibuka dari menu **Open config file**:

* Windows: `%APPDATA%\winmac\config.json`
* macOS: `~/Library/Application Support/winmac/config.json`

```json
{
  "mode": "server",
  "server_address": "",
  "remote_side": "right",
  "port": 24800,
  "swap_ctrl_cmd": true
}
```

Selepas edit fail secara manual, klik **Restart** dari menu. Log ada di `winmac.log` dalam folder yang sama (menu **Open log file**).

## CLI (tanpa tray)

Untuk debug, program juga boleh dijalankan dalam terminal; log dipaparkan terus:

```
winmac server [--remote-side left|right|top|bottom] [--port 24800] [--no-swap-ctrl-cmd]
winmac client <server-ip>[:port]
winmac --config-path
```

Jalankan `winmac` tanpa argumen untuk mod tray. Mod CLI tidak membaca fail config.

## Had (versi ini)

* Hanya **teks** dalam clipboard (bukan gambar atau fail).
* Andaian satu monitor setiap mesin (monitor utama).
* Tiada enkripsi: guna hanya dalam rangkaian rumah/pejabat yang dipercayai.
* Semasa mengawal mesin sebelah dari Windows, kursor Windows diletak di tengah skrinnya.

## Masalah biasa

| Masalah | Penyelesaian |
| --- | --- |
| Ikon oren, `Cannot reach ...` | Semak IP, kedua-dua mesin dalam rangkaian sama, firewall Windows membenarkan port 24800. |
| Ikon merah di Mac | Benarkan Accessibility (dan Input Monitoring untuk server) untuk WinMac, kemudian **Restart**. Jika masih merah selepas build semula, buang WinMac dari senarai Accessibility dan tambah semula. |
| `Cannot listen on port 24800` | WinMac sudah berjalan (semak tray), atau program lain guna port itu. |
| Kursor tidak menyeberang | Pastikan sisi skrin betul dan ikon hijau di kedua-dua mesin. |
| Windows: `link.exe not found` | Guna `.\build.ps1` (toolchain GNU), bukan `cargo build` biasa. |
