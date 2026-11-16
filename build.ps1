# Build winmac untuk Windows (release) menggunakan toolchain Rust GNU.
#
# Toolchain GNU (x86_64-pc-windows-gnu) membawa linker MinGW sendiri, jadi
# tidak perlu pasang Visual Studio / "Build Tools for Visual Studio" (link.exe).
$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot

$toolchain = "stable-x86_64-pc-windows-gnu"

if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    Write-Error "rustup tidak dijumpai. Pasang Rust dari https://rustup.rs dan buka semula PowerShell."
    exit 1
}

# Pasang toolchain GNU jika belum ada (hanya kali pertama).
$installed = rustup toolchain list | Select-String -SimpleMatch $toolchain
if (-not $installed) {
    Write-Host "Memasang toolchain $toolchain ..."
    rustup toolchain install $toolchain --profile minimal
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

cargo "+$toolchain" build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$bin = Join-Path $PSScriptRoot "target\release\winmac.exe"
Write-Host ""
Write-Host "Siap: $bin"
Write-Host "Jalankan server:  $bin server --remote-side right"
Write-Host "Jalankan client:  $bin client <IP-Mac>"
