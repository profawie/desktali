# desktali

A small Rust app to use **one mouse (and keyboard)** across two computers, Windows and macOS, on the same LAN/Wi-Fi. It does two things:

1. **Move the mouse across screens.** Push the cursor past the edge of the Windows screen and it appears on the MacBook. Move it back and it returns to Windows. Clicks, scrolling and the keyboard follow whichever machine is being controlled.
2. **Shared clipboard.** Copy text, an image, or files/folders on one machine and paste them on the other (both directions). When controlling the Mac from the Windows keyboard, `Ctrl+C` / `Ctrl+V` are automatically turned into `Cmd+C` / `Cmd+V`.
3. **Browse the other computer's files.** **Files (Mac)...** in the Windows tray menu (or **Files (Windows)...** in the Mac menu bar) opens a file browser in your web browser, showing the other computer's folders. Open, view or download any file.

The app runs as an icon in the **system tray** (Windows, next to the clock) and the **menu bar** (macOS). The icon colour shows the status:

* 🟢 green: connected
* 🟠 amber: waiting / trying to connect
* 🔴 red: needs attention (macOS permission, server address not set, port in use)

Click the icon for the menu: full status, **Files (Mac) / Files (Windows)**, **Mode** (Server / Client), **Other screen is on the** (Left / Right / Top / Bottom), **Server address...**, open the config / log file, Restart and Quit. Changing a setting saves the config and restarts the app automatically.

## How it works

* **Server**: the machine with the physical mouse (usually the Windows laptop).
* **Client**: the machine being controlled (usually the MacBook).

The client connects to the server over TCP port `24800`. The server installs a global input hook; when the cursor reaches the configured screen edge, all mouse/keyboard input is blocked locally and sent to the client. The clipboard is checked every 0.4 seconds on both sides and sent when it changes.

### Clipboard details

* **Text**: sent as is.
* **Images** (e.g. a screenshot, or "Copy image" in a browser): sent as PNG and placed on the other clipboard as an image, ready to paste into apps.
* **Files and folders** (copied in Explorer / Finder): their contents are sent, saved on the other machine in a temporary folder (`%TEMP%\desktali-clipboard` on Windows, `$TMPDIR/desktali-clipboard` on macOS), and put on the clipboard as files. Paste them in Explorer / Finder (or into an app) as usual. Only the latest batch is kept; older received files are deleted when new ones arrive.
* Files are sent only up to **100 MB** in total (and 2000 files). Larger copies are skipped and logged.
* While a large image or file batch is being sent, the mouse on the other machine may pause briefly.

### Browsing the other computer's files

Click **Files (Mac)...** in the Windows tray menu, or **Files (Windows)...** in the Mac menu bar. A page opens in your default browser:

* It starts with **Home**, **Desktop**, **Documents**, **Downloads** and the drives (Windows) or `/` and volumes (Mac). Click a folder to open it, **Up** to go back, or type a path and press Enter. **Hidden files** shows dotfiles / hidden files.
* Click a file name (or **Open**) to copy it to `Downloads/desktali` on this computer and open it with its default app. **View** shows it in the browser (images, PDF, text, video), **Download** saves it with the browser.
* Errors are shown at the top of the page, for example when a folder is not allowed.

**The first time**, the computer being browsed asks: *"... wants to browse and download files on this computer. Allow it?"*. Move the mouse over to that screen and click **Allow** (Yes on Windows). The answer is remembered for that peer in `file_browsing_peers` in its config file; remove the entry there to be asked again. To turn the feature off completely on a computer, set `"allow_file_browsing": false` in its config file.

**macOS privacy:** macOS protects Desktop, Documents, Downloads, iCloud Drive, external drives and some Library folders. The first time a folder is opened, the Mac may show its own prompt ("desktali would like to access files in your Documents folder"): click **Allow** on the Mac. If access was denied before, the page shows *"macOS blocked access to ..."*. To allow everything: **System Settings → Privacy & Security → Full Disk Access** → turn on **desktali** (or under **Files and Folders**), then **Restart** desktali on the Mac.

How it works: the page is served by desktali itself on `127.0.0.1` only (a random port, with a random secret in the URL), so other devices on the network cannot use it. Requests go to the other computer over the existing desktali connection; files are sent in 512 KB pieces, so the mouse keeps working during large downloads. Browsing is read-only: nothing can be changed or deleted on the other computer.

## Build

```sh
git clone https://github.com/profawie/desktali.git
cd desktali
```

* **Windows** (PowerShell): `.\build.ps1`
  (if PowerShell blocks the script: `powershell -ExecutionPolicy Bypass -File .\build.ps1`)
  Output: `desktali.exe` in the repository root (also in `target\release\`)
* **macOS**: `./build.sh`
  Output: `desktali.app` (menu bar app) and `desktali` (CLI) in the repository root (also in `target/release/`)

On Windows, `build.ps1` uses the Rust **GNU** toolchain (`stable-x86_64-pc-windows-gnu`) and installs it on first run. It does not need Visual Studio, MinGW or `dlltool`.

## Usage (mouse on Windows, MacBook on the right)

### 1. Windows (server)

1. Double-click `desktali.exe` in the repository folder. The icon appears in the system tray (it may be hidden under the `^` arrow).
2. The default mode on Windows is **Server**, with the other screen on the **Right**. Change it from the menu if needed.
3. Hover over the icon: the tooltip shows the Windows IP, e.g. `Waiting for client on 192.168.1.10:24800`.
4. The first time, Windows Firewall will ask; allow it for **Private networks**. If it does not ask (PowerShell as Administrator):

   ```powershell
   New-NetFirewallRule -DisplayName "desktali" -Direction Inbound -Protocol TCP -LocalPort 24800 -Action Allow -Profile Private
   ```

### 2. MacBook (client)

1. Open `desktali.app` in the repository folder (you can copy it to `/Applications`). The icon appears in the menu bar.
2. The default mode on the Mac is **Client**. Click the icon → **Server address...** → enter the Windows IP (e.g. `192.168.1.10`).
3. macOS will ask for **Accessibility** permission: open **System Settings → Privacy & Security → Accessibility**, turn on **desktali**, then click the icon → **Restart**.

When both icons are green, move the Windows mouse past the right edge of the screen.

### The other way round: mouse on the Mac

On the Mac choose **Mode → Server** and the side where the Windows screen is. On Windows choose **Mode → Client** and set **Server address...** to the Mac's IP. A Mac server needs both **Accessibility** and **Input Monitoring** permission.

### Start automatically at login

* Windows: press `Win+R`, type `shell:startup`, and put a shortcut to `desktali.exe` in that folder.
* macOS: **System Settings → General → Login Items** → `+` → choose `desktali.app`.

## Configuration

Config file (JSON), which you can open from the **Open config file** menu item:

* Windows: `%APPDATA%\desktali\config.json`
* macOS: `~/Library/Application Support/desktali/config.json`

```json
{
  "mode": "server",
  "server_address": "",
  "remote_side": "right",
  "port": 24800,
  "swap_ctrl_cmd": true,
  "allow_file_browsing": true,
  "file_browsing_peers": []
}
```

* `allow_file_browsing`: let the other computer browse this computer's files (it is still asked for once per peer).
* `file_browsing_peers`: peers that were allowed to browse files here (the server stores the client's IP; the client stores the server address).

After editing the file by hand, click **Restart** in the menu. The log is `desktali.log` in the same folder (menu **Open log file**).

## CLI (no tray)

For debugging, the app can also run in a terminal, with the log printed directly:

```
desktali server [--remote-side left|right|top|bottom] [--port 24800] [--no-swap-ctrl-cmd]
desktali client <server-ip>[:port]
desktali --config-path
```

Run `desktali` with no arguments for tray mode. CLI mode does not read the config file.

## Limitations

* Copied files are limited to 100 MB in total; they are sent in full when copied, not when pasted.
* Windows: all monitors are used; the cursor crosses at the outer edge of the whole desktop (e.g. the right edge of the right-most monitor). macOS: only the main display is used.
* No encryption (also for browsed files): only use it on a home/office network you trust.
* While controlling the other machine from Windows, the Windows cursor is parked in the middle of the monitor it left from.

## Troubleshooting

| Problem | Fix |
| --- | --- |
| Amber icon, `Cannot reach ...` | Check the IP, that both machines are on the same network, and that Windows Firewall allows port 24800. |
| Red icon on the Mac | Allow Accessibility (and Input Monitoring for server mode) for desktali, then **Restart**. If it stays red after a rebuild, remove desktali from the Accessibility list and add it again. |
| `Cannot listen on port 24800` | desktali is already running (check the tray), or another program uses that port. |
| Files copied on the Mac do not paste on Windows | macOS blocks reading files in Desktop, Documents, Downloads and similar folders. Allow desktali in **System Settings → Privacy & Security → Full Disk Access** (or **Files and Folders**), then **Restart**. The log shows `Operation not permitted` when this is the cause. |
| Files page: `Not connected to the other computer` | Both icons must be green. Reload the page after they are. |
| Files page: `No answer from the Mac/Windows` | Look at the other screen: it may be asking to allow access (desktali or macOS prompt). Also make sure both computers run this desktali version. |
| Files page: `macOS blocked access to ...` | Allow desktali in **System Settings → Privacy & Security → Full Disk Access** (or **Files and Folders**) on the Mac, then **Restart** desktali there. |
| Cursor does not cross over | Make sure the screen side is correct and both icons are green. |
| Windows: `link.exe not found` | Use `.\build.ps1` (GNU toolchain), not plain `cargo build`. |
