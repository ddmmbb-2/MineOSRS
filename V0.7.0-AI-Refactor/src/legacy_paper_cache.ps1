<#
.SYNOPSIS
  Populate the verified official vanilla server JAR into legacy Paperclip's cache.
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File .\Fix-MineOSRS-LegacyPaper.ps1 -ServerDir 'D:\Minecraft\my-paper-server' -Version '1.12.1'
.NOTES
  Only writes to <ServerDir>\cache; never touches worlds, Paper JAR, or settings.
  Stops on API/download/checksum errors without leaving a final JAR.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ServerDir,
    [ValidatePattern('^\d+\.\d+(\.\d+)?$')]
    [string]$Version = '1.12.1'
)

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Assert-MojangUrl([string]$Url, [string[]]$AllowedHosts) {
    $u = [uri]$Url
    if ($u.Scheme -ne 'https' -or $u.Host -notin $AllowedHosts) {
        throw "Unexpected download host: $Url"
    }
}

$resolved = Resolve-Path -LiteralPath $ServerDir -ErrorAction Stop
$root = $resolved.ProviderPath
if (-not (Test-Path -LiteralPath $root -PathType Container)) {
    throw "Server directory does not exist: $root"
}
if (-not (Test-Path -LiteralPath (Join-Path $root 'server.jar') -PathType Leaf)) {
    throw 'server.jar not found; install the Paper core before starting.'
}

# After the first successful download, verify the local SHA-1 stamp before any API call.
# This permits subsequent launches while offline, without blindly trusting the JAR.
$cacheDir = Join-Path $root 'cache'
$dest = Join-Path $cacheDir ("mojang_{0}.jar" -f $Version)
$stamp = "$dest.sha1"
if ((Test-Path -LiteralPath $dest -PathType Leaf) -and (Test-Path -LiteralPath $stamp -PathType Leaf)) {
    $savedSha1 = ([System.IO.File]::ReadAllText($stamp)).Trim().ToLowerInvariant()
    if ($savedSha1 -match '^[a-f0-9]{40}$') {
        $cachedSha1 = (Get-FileHash -LiteralPath $dest -Algorithm SHA1).Hash.ToLowerInvariant()
        if ($cachedSha1 -eq $savedSha1) {
            Write-Host "Validated Mojang cache (offline-safe): $dest" -ForegroundColor Green
            return
        }
    }
    Write-Warning 'Cached JAR or SHA1 stamp does not match. Checking the official source.'
}

Write-Host "Checking official Mojang release metadata for $Version..."
$manifestUri = 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json'
$manifest = Invoke-RestMethod -Uri $manifestUri -TimeoutSec 60
$ver = $manifest.versions | Where-Object { $_.id -eq $Version -and $_.type -eq 'release' } | Select-Object -First 1
if ($null -eq $ver) {
    throw "Version $Version not found in Mojang official release manifest."
}
Assert-MojangUrl $ver.url @('piston-meta.mojang.com', 'launchermeta.mojang.com')
$metadata = Invoke-RestMethod -Uri $ver.url -TimeoutSec 60
$artifact = $metadata.downloads.server
if ($null -eq $artifact -or [string]::IsNullOrWhiteSpace($artifact.url) -or $artifact.sha1 -notmatch '^[a-fA-F0-9]{40}$') {
    throw "Mojang does not provide a downloadable server JAR for version $Version."
}
Assert-MojangUrl $artifact.url @('piston-data.mojang.com', 'launcher.mojang.com')
$expected = $artifact.sha1.ToLowerInvariant()

if (Test-Path -LiteralPath $dest -PathType Leaf) {
    $current = (Get-FileHash -LiteralPath $dest -Algorithm SHA1).Hash.ToLowerInvariant()
    if ($current -eq $expected) {
        [System.IO.File]::WriteAllText($stamp, $expected, [System.Text.Encoding]::ASCII)
        Write-Host "Verified cache already exists: $dest" -ForegroundColor Green
        return
    }
    Write-Warning "Existing cache has an invalid SHA1. It will be preserved in a .bak file before replacement."
}

New-Item -ItemType Directory -Path $cacheDir -Force | Out-Null
$temp = Join-Path $cacheDir ("mojang_{0}.partial.{1}" -f $Version, $PID)
try {
    Write-Host "Downloading Minecraft $Version server JAR from Mojang..."
    Invoke-WebRequest -Uri $artifact.url -OutFile $temp -UseBasicParsing -TimeoutSec 180
    if ((Get-Item -LiteralPath $temp).Length -lt 1048576) {
        throw 'Downloaded JAR is suspiciously small; aborting.'
    }
    $actual = (Get-FileHash -LiteralPath $temp -Algorithm SHA1).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        throw "SHA1 mismatch! Expected $expected, received $actual. Not installing."
    }
    if (Test-Path -LiteralPath $dest -PathType Leaf) {
        $backup = "{0}.bak.{1}" -f $dest, (Get-Date -Format 'yyyyMMdd-HHmmss')
        Move-Item -LiteralPath $dest -Destination $backup -ErrorAction Stop
        Write-Host "Previous cache backed up to: $backup"
    }
    Move-Item -LiteralPath $temp -Destination $dest -ErrorAction Stop
    [System.IO.File]::WriteAllText($stamp, $expected, [System.Text.Encoding]::ASCII)
    Write-Host "Official file verified and cached: $dest" -ForegroundColor Green
    Write-Host "SHA1: $actual"
    Write-Host 'Legacy Paper cache is ready; continuing Minecraft server startup.'
}
finally {
    if (Test-Path -LiteralPath $temp) {
        Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue
    }
}
