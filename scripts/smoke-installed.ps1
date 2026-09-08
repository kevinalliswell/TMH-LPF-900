$ErrorActionPreference = 'Stop'
$installer = Get-ChildItem 'target/release/bundle/nsis/*.exe' | Select-Object -First 1
if (-not $installer) { throw 'No NSIS installer was produced' }
$installDir = 'C:\tmh-candidate-smoke'
$installed = Start-Process $installer.FullName -ArgumentList '/S', "/D=$installDir" -Wait -PassThru
if ($installed.ExitCode -ne 0) { throw "Installer failed: $($installed.ExitCode)" }
$binary = Join-Path $installDir 'tmh-app.exe'
if (-not (Test-Path $binary)) { throw 'Installed application binary is missing' }
$env:TMH_DEVICE_MODE = 'mock'
$env:TMH_DATA_DIR = Join-Path $env:RUNNER_TEMP 'tmh-candidate-installed-data'
$app = Start-Process $binary -PassThru
try {
  $deadline = (Get-Date).AddSeconds(45)
  do {
    Start-Sleep -Milliseconds 500
    $app.Refresh()
    if ($app.HasExited) { throw "Installed app exited before showing a window: $($app.ExitCode)" }
  } while ($app.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline)
  if ($app.MainWindowHandle -eq 0) { throw 'Installed app did not create a window in time' }
  $db = Join-Path $env:TMH_DATA_DIR 'data/experiments.db'
  if (-not (Test-Path $db)) { throw 'Installed app did not create its isolated database' }
  if (-not $app.CloseMainWindow()) { throw 'Installed app did not accept a normal close' }
  if (-not $app.WaitForExit(15000)) { throw 'Installed app did not finish closing' }
  if ($app.ExitCode -ne 0) { throw "Installed app close failed: $($app.ExitCode)" }
  python -c "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); assert c.execute('pragma integrity_check').fetchone()[0]=='ok'; assert c.execute('pragma user_version').fetchone()[0]==8; assert c.execute('select count(*) from users').fetchone()[0]==0; print('Installed app: valid schema 8, fresh bootstrap profile')" $db
  if ($LASTEXITCODE -ne 0) { throw 'Installed app database verification failed' }
} finally {
  $app.Refresh()
  if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force }
}
$uninstaller = Get-ChildItem $installDir -Filter '*uninstall*.exe' | Select-Object -First 1
if (-not $uninstaller) { throw 'Uninstaller is missing' }
$removed = Start-Process $uninstaller.FullName -ArgumentList '/S' -Wait -PassThru
if ($removed.ExitCode -ne 0) { throw "Uninstall failed: $($removed.ExitCode)" }
if (Test-Path $binary) { throw 'Application binary still exists after uninstall' }
Write-Output 'NSIS install / first launch / normal close / database / uninstall smoke passed'
