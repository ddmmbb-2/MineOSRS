param(
    [string] $SourceData = "",
    [string] $DestinationData = ""
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
if (-not $DestinationData) {
    if (-not $env:LOCALAPPDATA) { throw '找不到 LOCALAPPDATA，請用 -DestinationData 明確指定目錄。' }
    $DestinationData = Join-Path $env:LOCALAPPDATA 'MineOS-Win'
}
if (-not $SourceData) {
    $candidates = @(
        (Join-Path $projectRoot 'target\debug\data'),
        (Join-Path $projectRoot 'target\release\data')
    ) | Where-Object { Test-Path -LiteralPath (Join-Path $_ 'mineos.db') }
    if (@($candidates).Count -ne 1) {
        throw '找不到唯一的舊 data 資料夾；請手動指定 -SourceData "D:\...\target\debug\data"（必須有 mineos.db）。'
    }
    $SourceData = $candidates[0]
}
$source = [System.IO.Path]::GetFullPath($SourceData)
$dest = [System.IO.Path]::GetFullPath($DestinationData)
if ($source.TrimEnd([char]92) -eq $dest.TrimEnd([char]92)) { throw '來源與目標資料夾相同，不需要搬移。' }
if (-not (Test-Path -LiteralPath (Join-Path $source 'mineos.db'))) {
    throw "來源沒有 mineos.db：$source"
}
if (-not (Test-Path -LiteralPath (Join-Path $source 'admin-token.txt'))) {
    throw "來源沒有 admin-token.txt：$source"
}
if (Test-Path -LiteralPath $dest) {
    throw "目標資料夾已存在，為避免覆蓋資料已停止：$dest`n若你已啟動過 V0.3，請先停止程式，確認新資料沒有需要保存的內容，再自行備份與處理目標資料夾。"
}
Write-Host '請先完全關閉 MineOS-Win、停止所有 Minecraft Server，再執行搬移。' -ForegroundColor Yellow
Write-Host "來源：$source"
Write-Host "目標：$dest"
$answer = Read-Host '是否已確認所有伺服器停止且程式關閉？輸入 YES 繼續'
if ($answer -cne 'YES') { throw '已取消，資料沒有修改。' }
$parent = Split-Path $dest -Parent
if (-not (Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
Copy-Item -LiteralPath $source -Destination $dest -Recurse -ErrorAction Stop
if (-not (Test-Path -LiteralPath (Join-Path $dest 'mineos.db'))) {
    throw '複製後未找到 mineos.db；請檢查目標資料夾。舊資料仍保留。'
}
Write-Host '複製完成！舊 data 已保留，請啟動 V0.3 檢查 Server 清單和世界存檔。' -ForegroundColor Green
