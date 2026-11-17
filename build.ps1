# Build rtetherfy for Windows (release) with the GNU Rust toolchain.
#
# The GNU toolchain (x86_64-pc-windows-gnu) ships its own MinGW linker, so Visual
# Studio / "Build Tools for Visual Studio" (link.exe) is not needed.
$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot

$toolchain = "stable-x86_64-pc-windows-gnu"

if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    Write-Error "rustup not found. Install Rust from https://rustup.rs and reopen PowerShell."
    exit 1
}

# Install the GNU toolchain if missing (first run only).
$installed = rustup toolchain list | Select-String -SimpleMatch $toolchain
if (-not $installed) {
    Write-Host "Installing toolchain $toolchain ..."
    rustup toolchain install $toolchain --profile minimal
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

cargo "+$toolchain" build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$built = Join-Path $PSScriptRoot "target\release\rtetherfy.exe"
$bin = Join-Path $PSScriptRoot "rtetherfy.exe"

# Copy the result to the repository root for easy access.
try {
    Copy-Item -Path $built -Destination $bin -Force
} catch {
    Write-Error "Cannot copy to $bin. If rtetherfy is running, quit it from the tray icon and run build.ps1 again."
    exit 1
}

Write-Host ""
Write-Host "Done (copied to the repository root): $bin"
Write-Host "Tray app:    double-click rtetherfy.exe (or run it without arguments)"
Write-Host "CLI server:  .\rtetherfy.exe server --remote-side right"
Write-Host "CLI client:  .\rtetherfy.exe client <mac-ip>"
