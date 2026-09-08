$ErrorActionPreference = 'Stop'

# Do not accept the single-instance plugin's visible but off-screen tool window.
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class CandidateWindow {
    private delegate bool EnumWindowCallback(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowCallback callback, IntPtr parameter);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", SetLastError = true)] private static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    public static IntPtr Find(uint processId, string expectedTitle) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, parameter) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner != processId || !IsWindowVisible(window)) return true;
            var title = new StringBuilder(512);
            GetWindowText(window, title, title.Capacity);
            if (title.ToString() != expectedTitle) return true;
            found = window;
            return false;
        }, IntPtr.Zero);
        return found;
    }
    public static bool Close(IntPtr window) {
        return PostMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero); // WM_CLOSE
    }
}
'@

$installer = Get-ChildItem 'target/release/bundle/nsis/*.exe' | Select-Object -First 1
if (-not $installer) { throw 'No NSIS installer was produced' }
$installDir = 'C:\tmh-candidate-smoke'

# NSIS detects running applications by binary name. A harmless renamed ping
# process proves candidate install/uninstall cannot kill the production name.
$guardDir = Join-Path $env:RUNNER_TEMP 'tmh-production-process-guard'
New-Item -ItemType Directory -Force $guardDir | Out-Null
$guardBinary = Join-Path $guardDir 'tmh-app.exe'
Copy-Item (Join-Path $env:SystemRoot 'System32/ping.exe') $guardBinary
$guard = Start-Process $guardBinary -ArgumentList '-t', '127.0.0.1' -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $guardDir 'ping.log')
$app = $null
try {
  Start-Sleep -Milliseconds 500
  $guard.Refresh()
  if ($guard.HasExited) { throw 'Production-name process guard could not start' }
  $installed = Start-Process $installer.FullName -ArgumentList '/S', "/D=$installDir" -Wait -PassThru
  if ($installed.ExitCode -ne 0) { throw "Installer failed: $($installed.ExitCode)" }
  $guard.Refresh()
  if ($guard.HasExited) { throw 'Candidate installation terminated the production-name process' }
  $binary = Join-Path $installDir 'tmh-app-candidate.exe'
  if (-not (Test-Path $binary)) { throw 'Installed application binary is missing' }
  $env:TMH_DEVICE_MODE = 'mock'
  $env:TMH_DATA_DIR = $null
  $profileRoot = [Environment]::GetFolderPath('ApplicationData')
  $dataRoot = Join-Path $profileRoot 'tmh-lpf-900-candidate'
  $productionData = Join-Path $profileRoot 'tmh-lpf-900'
  if ((Test-Path $dataRoot) -or (Test-Path $productionData)) { throw 'Installed smoke requires clean default profiles' }
  $db = Join-Path $dataRoot 'data/experiments.db'
  $app = Start-Process $binary -PassThru
  $deadline = (Get-Date).AddSeconds(45)
  $mainWindow = [IntPtr]::Zero
  $databaseReady = $false
  do {
    Start-Sleep -Milliseconds 500
    $app.Refresh()
    if ($app.HasExited) { throw "Installed app exited before becoming ready: $($app.ExitCode)" }
    $mainWindow = [CandidateWindow]::Find($app.Id, 'TMH-LPF-900 候选测试版')
    if ($mainWindow -ne [IntPtr]::Zero -and (Test-Path $db)) {
      python -c "import sqlite3,sys; c=sqlite3.connect(sys.argv[1],timeout=1); sys.exit(0 if c.execute('pragma user_version').fetchone()[0]==8 else 1)" $db
      $databaseReady = $LASTEXITCODE -eq 0
    }
  } while (($mainWindow -eq [IntPtr]::Zero -or -not $databaseReady) -and (Get-Date) -lt $deadline)
  if ($mainWindow -eq [IntPtr]::Zero -or -not $databaseReady) { throw 'Installed app did not create its main window and initialize its database in time' }
  if (-not [CandidateWindow]::Close($mainWindow)) { throw 'Installed app did not accept a normal close' }
  if (-not $app.WaitForExit(15000)) { throw 'Installed app did not finish closing' }
  if ($app.ExitCode -ne 0) { throw "Installed app close failed: $($app.ExitCode)" }
  python -c "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); assert c.execute('pragma integrity_check').fetchone()[0]=='ok'; assert c.execute('pragma user_version').fetchone()[0]==8; assert c.execute('select count(*) from users').fetchone()[0]==0; print('Installed app: valid schema 8, fresh bootstrap profile')" $db
  if ($LASTEXITCODE -ne 0) { throw 'Installed app database verification failed' }
  if (Test-Path $productionData) { throw 'Candidate launch touched the production data profile' }
  $uninstaller = Get-ChildItem $installDir -Filter '*uninstall*.exe' | Select-Object -First 1
  if (-not $uninstaller) { throw 'Uninstaller is missing' }
  # NSIS otherwise starts a temporary child uninstaller and exits before removal.
  $removed = Start-Process $uninstaller.FullName -ArgumentList '/S', "_?=$installDir" -Wait -PassThru
  if ($removed.ExitCode -ne 0) { throw "Uninstall failed: $($removed.ExitCode)" }
  if (Test-Path $binary) { throw 'Application binary still exists after uninstall' }
  $guard.Refresh()
  if ($guard.HasExited) { throw 'Candidate uninstall terminated the production-name process' }
  Write-Output 'NSIS install / main window / normal close / database / uninstall / production process isolation smoke passed'
} finally {
  if ($null -ne $app) {
    $app.Refresh()
    if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force }
  }
  $guard.Refresh()
  if (-not $guard.HasExited) { Stop-Process -Id $guard.Id -Force }
}
