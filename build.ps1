# Build winmac untuk Windows (release).
$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "cargo tidak dijumpai. Pasang Rust: https://rustup.rs"
    exit 1
}

cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$bin = Join-Path $PSScriptRoot "target\release\winmac.exe"
Write-Host ""
Write-Host "Siap: $bin"
Write-Host "Jalankan server:  $bin server --remote-side right"
Write-Host "Jalankan client:  $bin client <IP-Mac>"
