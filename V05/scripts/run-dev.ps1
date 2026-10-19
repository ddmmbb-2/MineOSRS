$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "找不到 cargo。請先安裝 Rust (https://rustup.rs/) 並開啟新 PowerShell。"
}
Write-Host "啟動 MineOS-Win (http://127.0.0.1:3000)" -ForegroundColor Cyan
cargo run --release
