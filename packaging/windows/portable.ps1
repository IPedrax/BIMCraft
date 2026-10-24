# The portable build: bimcraft.exe, bimcraft-cli.exe, the licences and the readme in one zip, to unzip anywhere and run, for a PC
# whose user can't install software. Then it unzips the zip somewhere else and runs what is in it, as a user would.
#
#   packaging\windows\portable.ps1 -Version 0.8.0 -BinDir target\x86_64-pc-windows-msvc\release
#
# The zip is bimcraft-<version>-windows-x64-portable.zip in the current folder (its name goes to PORTABLE in $GITHUB_ENV, in
# Actions). It isn't one of the files the updater looks for (`Target::asset_name`). Its settings still go to %APPDATA%\BIMCraft,
# which needs no rights, and the updater only replaces the installer's copy in Program Files, so a portable copy is told of a
# newer version with a Download button.
param(
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][string]$BinDir
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$bin = (Resolve-Path $BinDir).Path
$name = "bimcraft-$Version-windows-x64-portable"
$work = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
$stage = Join-Path (Join-Path $work 'portable-stage') $name
$zip = Join-Path (Get-Location) "$name.zip"

Remove-Item (Split-Path $stage) -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item $zip -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $stage | Out-Null
Copy-Item "$bin\bimcraft.exe", "$bin\bimcraft-cli.exe", "$root\LICENSE-MIT", "$root\LICENSE-APACHE", "$root\README.md" $stage
Compress-Archive -Path $stage -DestinationPath $zip

# What a user gets: the zip opened in a folder of its own, and the programs in it run from there.
$opened = Join-Path $work 'portable-opened'
Remove-Item $opened -Recurse -Force -ErrorAction SilentlyContinue
Expand-Archive $zip -DestinationPath $opened
$inside = Join-Path $opened $name
foreach ($file in 'bimcraft.exe', 'bimcraft-cli.exe', 'LICENSE-MIT', 'LICENSE-APACHE', 'README.md') {
  if (-not (Test-Path (Join-Path $inside $file))) { throw "expected $file in $name.zip" }
}
$said = & "$inside\bimcraft-cli.exe" --version
if ($said -ne "bimcraft-cli $Version") { throw "expected 'bimcraft-cli $Version' from the unzipped CLI, got '$said'" }
# The window's program has no console, so what it prints goes nowhere: its exit code is what says it ran.
$app = Start-Process "$inside\bimcraft.exe" -ArgumentList '--version' -Wait -PassThru
if ($app.ExitCode -ne 0) { throw "the unzipped bimcraft.exe --version exited with $($app.ExitCode)" }
Remove-Item $opened -Recurse -Force
Write-Host "ok: $name.zip runs from a folder it was unzipped into"

if ($env:GITHUB_ENV) { Add-Content -Path $env:GITHUB_ENV -Value "PORTABLE=$name.zip" }
