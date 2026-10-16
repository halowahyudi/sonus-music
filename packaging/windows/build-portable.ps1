# Builds the Sonus portable zip for Windows.
#
# The zip contains sonus.exe only. yt-dlp and ffmpeg are downloaded into the
# user's data dir on first launch, so the portable archive stays tiny.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File build-portable.ps1
#
# Requires: the release binary already built (cargo build --release).

$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Version = "0.3.0"
$Dist = Join-Path $PSScriptRoot "dist"
$Stage = Join-Path $Dist "sonus-portable"

# Read the version from the workspace Cargo.toml so we don't hardcode it twice.
$Cargo = Get-Content (Join-Path $Root "Cargo.toml") | Select-String '^version\s*=\s*"([^"]+)"'
if ($Cargo.Matches.Count -gt 0) {
    $Version = $Cargo.Matches[0].Groups[1].Value
}

$Binary = Join-Path $Root "target\release\sonus.exe"
if (-not (Test-Path $Binary)) {
    throw "Release binary not found: $Binary. Run `cargo build --release` first."
}

if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Path $Stage | Out-Null

Copy-Item $Binary (Join-Path $Stage "sonus.exe")
Copy-Item (Join-Path $Root "LICENSE") (Join-Path $Stage "LICENSE")

# A tiny README so portable users know where their data lives.
@"
Sonus portable
==============
Just run sonus.exe. It downloads yt-dlp and ffmpeg into your user data folder
on first launch:

    %LOCALAPPDATA%\sonus\bin

To update the bundled tools later:  sonus deps update
To see where everything lives:      sonus paths
"@ | Set-Content (Join-Path $Stage "README.txt") -Encoding UTF8

$Zip = Join-Path $Dist "sonus-$Version-portable.zip"
if (Test-Path $Zip) { Remove-Item -Force $Zip }
Compress-Archive -Path (Join-Path $Stage "*") -DestinationPath $Zip

Write-Host "Portable archive: $Zip"
