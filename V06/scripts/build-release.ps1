$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "找不到 cargo。請先安裝 Rust (https://rustup.rs/) 並開啟新 PowerShell。"
}
cargo build --release
Write-Host "完成：target\release\mineos-win.exe" -ForegroundColor Green
Write-Host "請把 EXE 放在可寫入的資料夾中執行；將於旁邊建立 data 資料夾。" -ForegroundColor Yellow
