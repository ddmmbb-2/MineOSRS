$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "找不到 cargo。請先安裝 Rust (https://rustup.rs/) 並開啟新 PowerShell。"
}
cargo build --release
Write-Host "完成：target\release\mineos-win.exe" -ForegroundColor Green
Write-Host "預設資料位置：%LOCALAPPDATA%\MineOS-Win；MINEOS_DATA_DIR 可指定其他資料夾。" -ForegroundColor Yellow
