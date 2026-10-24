# Checks the MSI's setup wizard leads Welcome > Folder > Shortcuts > Ready (read from the MSI's tables: a script can't click
# through it), then installs it into a folder of the user's choosing, as the folder page does, upgrades it with a second
# build of the same source, as an update does, and checks the folder is kept; then removes it. Needs an administrator
# (the installer is for every user), and installs for real: use a CI runner or a throwaway machine.
#
#   packaging\windows\check-install.ps1 -Msi bimcraft-0.8.0-windows-x64.msi -Upgrade rebuilt.msi
#
# A rebuild of the same version has a product code of its own and the same upgrade code, so Windows Installer takes it for
# an upgrade (the installer allows same-version upgrades).
param(
  [Parameter(Mandatory)][string]$Msi,
  [Parameter(Mandatory)][string]$Upgrade
)
$ErrorActionPreference = 'Stop'
$msiPath = (Resolve-Path $Msi).Path
$upgradePath = (Resolve-Path $Upgrade).Path
$work = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
# Not Program Files, and with a space in the name, as a folder somebody types would have.
$dir = Join-Path $work 'BIMCraft elsewhere'
$programFiles = Join-Path $env:ProgramFiles 'BIMCraft'
$log = Join-Path $work 'check-install.log'

function Run-Msiexec([string]$what) {
  # 3010 is "done, restart needed", which nothing here needs but isn't a failure.
  $p = Start-Process msiexec.exe -ArgumentList "$what /qn /l*v `"$log`"" -Wait -PassThru
  if ($p.ExitCode -notin 0, 3010) { throw "msiexec $what exited with $($p.ExitCode)" }
}
function Expect([bool]$ok, [string]$what) { if (-not $ok) { throw "expected: $what" } }

# The rows of a query over the MSI, each with its columns joined by '|'.
function Rows([string]$sql, [int]$columns) {
  $installer = New-Object -ComObject WindowsInstaller.Installer
  $db = $installer.GetType().InvokeMember('OpenDatabase', 'InvokeMethod', $null, $installer, @($msiPath, 0))
  $view = $db.GetType().InvokeMember('OpenView', 'InvokeMethod', $null, $db, @($sql))
  [void]$view.GetType().InvokeMember('Execute', 'InvokeMethod', $null, $view, $null)
  while ($rec = $view.GetType().InvokeMember('Fetch', 'InvokeMethod', $null, $view, $null)) {
    (1..$columns | ForEach-Object { $rec.GetType().InvokeMember('StringData', 'GetProperty', $null, $rec, @([int]$_)) }) -join '|'
  }
  [void]$view.GetType().InvokeMember('Close', 'InvokeMethod', $null, $view, $null)
}

$events = @(Rows 'SELECT `Dialog_`, `Control_`, `Event`, `Argument` FROM `ControlEvent`' 4)
foreach ($step in 'WelcomeDlg|Next|NewDialog|InstallDirDlg', 'InstallDirDlg|Back|NewDialog|WelcomeDlg',
    'InstallDirDlg|Next|SetTargetPath|[WIXUI_INSTALLDIR]', 'InstallDirDlg|Next|NewDialog|ShortcutsDlg',
    'InstallDirDlg|ChangeFolder|SpawnDialog|BrowseDlg', 'ShortcutsDlg|Back|NewDialog|InstallDirDlg',
    'ShortcutsDlg|Next|NewDialog|VerifyReadyDlg', 'VerifyReadyDlg|Back|NewDialog|ShortcutsDlg') {
  Expect ($events -contains $step) "the wizard to have '$step'"
}
$dialogs = @(Rows 'SELECT `Dialog` FROM `Dialog`' 1)
foreach ($d in 'InstallDirDlg', 'BrowseDlg', 'InvalidDirDlg') { Expect ($dialogs -contains $d) "the $d dialog" }
Expect (@(Rows 'SELECT `Action` FROM `CustomAction`' 1) -contains 'WixUIValidatePath_X64') 'the path check'
Expect (@(Rows 'SELECT `Property`, `Value` FROM `Property`' 2) -contains 'WIXUI_INSTALLDIR|INSTALLFOLDER') 'the folder page to edit INSTALLFOLDER'

try {
  Run-Msiexec "/i `"$msiPath`" INSTALLFOLDER=`"$dir`""
  Expect (Test-Path "$dir\bimcraft.exe") 'bimcraft.exe in the folder that was chosen'
  Expect (Test-Path "$dir\bimcraft-cli.exe") 'bimcraft-cli.exe in the folder that was chosen'
  Expect (-not (Test-Path $programFiles)) 'nothing in Program Files'
  $appPath = (Get-Item 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\bimcraft.exe').GetValue('')
  Expect ($appPath -eq "$dir\bimcraft.exe") "App Paths names the chosen folder, not '$appPath'"

  # No folder given this time: the upgrade has to find where BIMCraft is.
  Run-Msiexec "/i `"$upgradePath`""
  Expect (Test-Path "$dir\bimcraft.exe") 'the upgrade kept the chosen folder'
  Expect (-not (Test-Path $programFiles)) 'the upgrade did not move BIMCraft to Program Files'

  Run-Msiexec "/x `"$upgradePath`""
  Expect (-not (Test-Path "$dir\bimcraft.exe")) 'the uninstall removed the program'
  Expect (-not (Test-Path 'HKLM:\SOFTWARE\BIMCraft')) 'the uninstall removed the remembered folder'
  Write-Host "ok: installed to '$dir', kept it through an upgrade, removed it"
} catch {
  Write-Host "msiexec log, last lines:"
  if (Test-Path $log) { Get-Content $log -Tail 60 }
  # Leave nothing installed behind a failure.
  foreach ($m in $upgradePath, $msiPath) { Start-Process msiexec.exe -ArgumentList "/x `"$m`" /qn" -Wait }
  throw
}
