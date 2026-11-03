param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [string]$OutputDir = (Join-Path $env:RUNNER_TEMP "rongguang-smoke")
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force $OutputDir | Out-Null
$installDir = Join-Path $OutputDir "install"
$installerProcess = Start-Process -FilePath (Resolve-Path $Installer) -ArgumentList @("/S", "/D=$installDir") -PassThru -Wait
if ($installerProcess.ExitCode -ne 0) { throw "Installer failed: $($installerProcess.ExitCode)" }
$executable = Join-Path $installDir "rongguang.exe"
if (-not (Test-Path $executable)) { throw "Installed executable missing: $executable" }

function Read-AppLog([string]$DataDir) {
    $files = Get-ChildItem (Join-Path $DataDir "logs") -Filter "rongguang.log*" -ErrorAction SilentlyContinue
    return (($files | ForEach-Object { Get-Content $_.FullName -Raw -Encoding utf8 }) -join "`n")
}

function Wait-App([System.Diagnostics.Process]$App, [scriptblock]$Condition) {
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    while ([DateTime]::UtcNow -lt $deadline) {
        $App.Refresh()
        if ($App.HasExited) { throw "App exited unexpectedly: $($App.ExitCode)" }
        if (& $Condition) {
            if ($App.MainWindowHandle -eq 0 -or -not $App.Responding) { throw "Main window is missing or not responding" }
            return
        }
        Start-Sleep -Milliseconds 500
    }
    throw "App startup timed out"
}

function Stop-App([System.Diagnostics.Process]$App) {
    if ($null -ne $App -and -not $App.HasExited) {
        $closed = $App.CloseMainWindow()
        if (-not $closed -or -not $App.WaitForExit(10000)) {
            Stop-Process -Id $App.Id -Force -ErrorAction SilentlyContinue
            throw "App did not close normally"
        }
    }
}

$oldDataDir = $env:FUSION_DATA_DIR
$oldPort = $env:FUSION_PORT
$oldWebDir = $env:FUSION_WEB_DIR
$app = $null
$listener = $null
try {
    # Exercise the installed resources, not the source checkout's web/out.
    $env:FUSION_WEB_DIR = Join-Path $installDir "web"
    $env:FUSION_PORT = "0"
    $env:FUSION_DATA_DIR = Join-Path $OutputDir "data"
    foreach ($attempt in 1..2) {
        $portFile = Join-Path $env:FUSION_DATA_DIR "runtime-port"
        Remove-Item $portFile -ErrorAction SilentlyContinue
        $app = Start-Process -FilePath $executable -WorkingDirectory $installDir -PassThru
        Wait-App $app {
            (Test-Path $portFile) -and $app.MainWindowTitle -eq "融光 · AI 视频创作工作台"
        }
        $port = (Get-Content $portFile -Raw).Trim()
        $health = Invoke-WebRequest "http://127.0.0.1:$port/api/health" -TimeoutSec 5
        if ($health.Content.Trim() -ne "ok") { throw "Backend health check failed" }
        $page = Invoke-WebRequest "http://127.0.0.1:$port/dashboard/" -TimeoutSec 5
        if ($page.StatusCode -ne 200 -or $page.Content -notmatch "_next/") { throw "Dashboard resources missing" }
        if (-not (Test-Path (Join-Path $env:FUSION_DATA_DIR "fusion.db"))) { throw "SQLite database missing" }
        if ((Read-AppLog $env:FUSION_DATA_DIR) -notmatch "主界面加载完成") { throw "WebView did not finish loading the dashboard" }
        # The original tray deadlock occurred after backend startup; keep probing the UI.
        foreach ($probe in 1..10) {
            Start-Sleep -Milliseconds 500
            $app.Refresh()
            if ($app.HasExited -or -not $app.Responding) { throw "App froze after startup" }
        }
        Stop-App $app
        $app = $null
        Write-Host "PASS: installed app launch $attempt, WebView navigation, responsive UI, backend, SQLite, normal close"
    }

    # Bind failure must show a usable error window, not leave a hidden/white window.
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $env:FUSION_PORT = $listener.LocalEndpoint.Port.ToString()
    $env:FUSION_DATA_DIR = Join-Path $OutputDir "port-conflict"
    $app = Start-Process -FilePath $executable -WorkingDirectory $installDir -PassThru
    Wait-App $app {
        $app.MainWindowTitle -eq "融光未能启动" -and
        (Read-AppLog $env:FUSION_DATA_DIR) -match "启动错误页面加载完成"
    }
    Stop-App $app
    $app = $null
    Write-Host "PASS: occupied port shows a responsive error page and closes normally"
} finally {
    if ($null -ne $listener) { $listener.Stop() }
    if ($null -ne $app -and -not $app.HasExited) { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue }
    $env:FUSION_DATA_DIR = $oldDataDir
    $env:FUSION_PORT = $oldPort
    $env:FUSION_WEB_DIR = $oldWebDir
}
