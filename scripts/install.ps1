[CmdletBinding()]
param([switch] $Launch)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$destination = Join-Path $env:LOCALAPPDATA 'Programs\Velune'
$executable = Join-Path $destination 'velune.exe'
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargo) {
    $cargo = Get-Command (Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe') -ErrorAction Stop
}

if (Get-Process -Name 'velune' -ErrorAction SilentlyContinue) {
    throw 'Exit Mouse Mover from its tray menu before installing an update.'
}

Push-Location $root
try {
    & $cargo.Source build --locked --release
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed; installation was not changed.' }
    [void] (New-Item -ItemType Directory -Path $destination -Force)
    Copy-Item (Join-Path $root 'target\release\velune.exe') $executable -Force
}
finally {
    Pop-Location
}

"Installed: $executable"
'Startup was not changed. To enable it, launch this installed copy and select Start with Windows in its tray menu.'
if ($Launch) { Start-Process -FilePath $executable }