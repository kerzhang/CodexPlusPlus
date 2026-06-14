#Requires -Version 5.1
<#
.SYNOPSIS
  Build Codex++ Windows release binaries and NSIS installer.

.DESCRIPTION
  Mirrors .github/workflows/release-assets.yml (windows-installer job):
    1. npm install + vite:build (manager frontend)
    2. cargo build --release (launcher + manager)
    3. stage exes under dist/windows/app
    4. makensis CodexPlusPlus.nsi

.PARAMETER Version
  Installer version string (e.g. 1.2.5). Defaults to workspace version in Cargo.toml.

.PARAMETER SkipFrontend
  Skip npm install and vite:build (use when frontend is already built).

.PARAMETER SkipInstaller
  Only build binaries; do not run NSIS.

.EXAMPLE
  .\scripts\build-windows.ps1

.EXAMPLE
  .\scripts\build-windows.ps1 -Version 1.2.5-dev -SkipFrontend
#>
[CmdletBinding()]
param(
  [string] $Version,
  [switch] $SkipFrontend,
  [switch] $SkipInstaller
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-WorkspaceVersion {
  param([string] $CargoTomlPath)
  $content = Get-Content -LiteralPath $CargoTomlPath -Raw
  if ($content -match '(?ms)\[workspace\.package\].*?^version\s*=\s*"([^"]+)"') {
    return $Matches[1]
  }
  throw "Could not read workspace version from $CargoTomlPath"
}

function Initialize-RustPath {
  $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
  $cargoExe = Join-Path $cargoBin 'cargo.exe'
  if (Test-Path -LiteralPath $cargoExe) {
    if ($env:PATH -notlike "*$cargoBin*") {
      $env:PATH = "$cargoBin;$env:PATH"
      Write-Host "  added to PATH: $cargoBin"
    }
    return (Resolve-Path -LiteralPath $cargoExe).Path
  }
  $cmd = Get-Command cargo -ErrorAction SilentlyContinue
  if ($cmd) {
    return $cmd.Source
  }
  throw @"
cargo not found.
Install Rust: https://rustup.rs/
After install, restart the terminal or run: refreshenv
"@
}

function Resolve-MakeNsis {
  $candidates = @(
    "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
    "${env:ProgramFiles}\NSIS\makensis.exe"
  )
  foreach ($path in $candidates) {
    if (Test-Path -LiteralPath $path) {
      return $path
    }
  }
  $cmd = Get-Command makensis -ErrorAction SilentlyContinue
  if ($cmd) {
    return $cmd.Source
  }
  throw @"
NSIS makensis.exe not found.
Install NSIS:
  winget install NSIS.NSIS
Or: https://nsis.sourceforge.io/Download
"@
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$ManagerDir = Join-Path $Root 'apps\codex-plus-manager'
$CargoToml = Join-Path $Root 'Cargo.toml'
$DistApp = Join-Path $Root 'dist\windows\app'
$NsisDir = Join-Path $Root 'scripts\installer\windows'

if (-not $Version) {
  $Version = Get-WorkspaceVersion -CargoTomlPath $CargoToml
}

Write-Host "Codex++ Windows build"
Write-Host "  root:    $Root"
Write-Host "  version: $Version"
Write-Host ""

if (-not $SkipFrontend) {
  Write-Host '==> Frontend: npm install'
  Push-Location $ManagerDir
  try {
    npm install
    Write-Host '==> Frontend: npm run vite:build'
    npm run vite:build
  }
  finally {
    Pop-Location
  }
}
else {
  Write-Host '==> Frontend: skipped (-SkipFrontend)'
}

$cargo = Initialize-RustPath
Write-Host "==> Rust: cargo build --release ($cargo)"
Push-Location $Root
try {
  & $cargo build --release -p codex-plus-launcher -p codex-plus-manager
}
finally {
  Pop-Location
}

$LauncherExe = Join-Path $Root 'target\release\codex-plus-plus.exe'
$ManagerExe = Join-Path $Root 'target\release\codex-plus-plus-manager.exe'
foreach ($exe in @($LauncherExe, $ManagerExe)) {
  if (-not (Test-Path -LiteralPath $exe)) {
    throw "Expected binary missing: $exe"
  }
}

Write-Host '==> Stage: dist/windows/app'
New-Item -ItemType Directory -Force -Path $DistApp | Out-Null
Copy-Item -LiteralPath $LauncherExe -Destination (Join-Path $DistApp 'codex-plus-plus.exe') -Force
Copy-Item -LiteralPath $ManagerExe -Destination (Join-Path $DistApp 'codex-plus-plus-manager.exe') -Force

$InstallerPath = Join-Path $Root "dist\windows\CodexPlusPlus-$Version-windows-x64-setup.exe"

if ($SkipInstaller) {
  Write-Host '==> NSIS: skipped (-SkipInstaller)'
}
else {
  $makensis = Resolve-MakeNsis
  Write-Host "==> NSIS: $makensis"
  Push-Location $NsisDir
  try {
    & $makensis '/INPUTCHARSET' 'UTF8' "/DVERSION=$Version" 'CodexPlusPlus.nsi'
  }
  finally {
    Pop-Location
  }
  if (-not (Test-Path -LiteralPath $InstallerPath)) {
    throw "Installer was not created: $InstallerPath"
  }
}

Write-Host ''
Write-Host 'Done.'
Write-Host "  launcher:  $LauncherExe"
Write-Host "  manager:   $ManagerExe"
if (-not $SkipInstaller) {
  Write-Host "  installer: $InstallerPath"
}
