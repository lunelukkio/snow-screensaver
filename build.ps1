# Builds the release executable and places it in dist/ (the folder to pin
# to the taskbar). Run from any directory:  ./build.ps1

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item 'target\release\snow.exe' 'dist\snow.exe' -Force

# The executable reads config.json from its own folder; carry existing
# settings over the first time.
if (-not (Test-Path 'dist\config.json') -and (Test-Path 'config.json')) {
    Copy-Item 'config.json' 'dist\config.json'
}

$size = (Get-Item 'dist\snow.exe').Length / 1KB
'Built dist\snow.exe ({0:N0} KB)' -f $size
