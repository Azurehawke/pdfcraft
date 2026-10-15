<#
.SYNOPSIS
  Build and package RevuCraft for Windows (x64).

.DESCRIPTION
  Produces, in $env:DIST (default: dist/revucraft):
    RevuCraft-setup.msi            per-machine installer (WiX v5): Start menu and desktop shortcuts,
                                   offered under "Open with" for PDFs, uninstall from Settings
    RevuCraft-portable.zip         revucraft.exe + revucraft-cli.exe + models\ + portable.txt

  The binaries are PdfCraft's (`pdfcraft`, `pdfcraft-cli`) built from the revucraft branch and
  shipped under RevuCraft's names; apps/pdfcraft/build.rs embeds RevuCraft's icon and name. The C
  runtime is linked statically, so no Visual C++ redistributable is needed. Unsigned.

  REVUCRAFT_BUILD (set by CI) is the build number the update check compares; the MSI version is
  1.0.<build>, so each build upgrades the one before.

  Needs: Rust (MSVC), the network (OCR models), the Windows SDK (rc.exe), and WiX v5:
  dotnet tool install --global wix --version 5.0.2

.EXAMPLE
  pwsh packaging/windows/revucraft-package.ps1
#>
param([switch] $SkipBuild)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

$Build = if ($env:REVUCRAFT_BUILD) { $env:REVUCRAFT_BUILD } else { '0' }
if ($Build -notmatch '^\d{1,5}$') { throw "REVUCRAFT_BUILD must be a number up to 65535, not '$Build'" }
$MsiVersion = "1.0.$Build"
$Target = 'x86_64-pc-windows-msvc'
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\revucraft' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

Write-Output "RevuCraft build $Build for Windows x64 (MSI $MsiVersion)"

if (-not $SkipBuild) {
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  $env:PDFCRAFT_REQUIRE_WINRES = '1'
  Invoke-Native "cargo build ($Target)" { cargo build --release -p pdfcraft -p pdfcraft-cli --target $Target }
}
$Bin = Join-Path $TargetDir "$Target\release"

# The app must be a GUI program (no console window) and the CLI a console one.
function Get-Subsystem([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return [BitConverter]::ToUInt16($bytes, $pe + 0x5C)
}
if ((Get-Subsystem (Join-Path $Bin 'pdfcraft.exe')) -ne 2) { throw 'pdfcraft.exe is not a GUI program' }
if ((Get-Subsystem (Join-Path $Bin 'pdfcraft-cli.exe')) -ne 3) { throw 'pdfcraft-cli.exe is not a console program' }

$Stage = Join-Path $TargetDir 'revucraft-package\stage'
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item (Join-Path $Bin 'pdfcraft.exe') (Join-Path $Stage 'revucraft.exe')
Copy-Item (Join-Path $Bin 'pdfcraft-cli.exe') (Join-Path $Stage 'revucraft-cli.exe')

# OCR models: the app looks in models\ beside the executable.
$Models = Join-Path $Stage 'models'
New-Item -ItemType Directory -Force -Path $Models | Out-Null
Push-Location $Root
try { Invoke-Native 'cargo xtask models' { cargo xtask models $Models } } finally { Pop-Location }
Remove-Item -Force (Join-Path $Models '*.part') -ErrorAction SilentlyContinue

# ---- MSI ---------------------------------------------------------------------------------------
$Msi = Join-Path $Dist 'RevuCraft-setup.msi'
Invoke-Native 'wix build' {
  wix build (Join-Path $PSScriptRoot 'revucraft.wxs') -arch x64 `
    (Join-Path $PSScriptRoot 'revucraft-installer-ui.wxs') `
    -d "Version=$MsiVersion" -d "BinDir=$Stage" -d "ModelsDir=$Models" -d "IconPath=$(Join-Path $Root 'assets\app-icon\revucraft.ico')" `
    -o $Msi
}
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))

# ---- portable zip ------------------------------------------------------------------------------
$Portable = Join-Path $TargetDir 'revucraft-package\RevuCraft'
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $Stage '*.exe') $Portable
Copy-Item -Recurse $Models (Join-Path $Portable 'models')
foreach ($f in 'REVUCRAFT.md', 'LICENSE-MIT', 'LICENSE-APACHE') {
  $p = Join-Path $Root $f
  if (Test-Path $p) { Copy-Item $p $Portable }
}
@"
RevuCraft portable mode

While this file sits next to revucraft.exe, RevuCraft keeps its settings, logs and crash-recovery
autosaves in the RevuCraftData folder beside it instead of in your Windows profile. Copy the whole
folder to move RevuCraft with its settings. Delete this file to use the normal per-user folders.
"@ | Set-Content -Encoding utf8 (Join-Path $Portable 'portable.txt')
$Zip = Join-Path $Dist 'RevuCraft-portable.zip'
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

Invoke-Native 'revucraft-cli --version' { & (Join-Path $Stage 'revucraft-cli.exe') --version }
Get-Item $Msi, $Zip | Format-Table Name, Length
