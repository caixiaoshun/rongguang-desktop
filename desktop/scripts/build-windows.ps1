# 融光桌面端 · Windows 打包脚本
#
# 前置依赖：
#   - Rust stable（MSVC 工具链）+ Visual Studio Build Tools(C++ 桌面开发)
#   - Node.js 20+ / pnpm
#   - WebView2 Runtime（Win10 1803+ 一般已内建）
#   - ffmpeg 预编译包：把 ffmpeg.exe / ffprobe.exe 放到 desktop\vendor\ffmpeg\ 下
#
# 用法（在 desktop 目录执行）：
#   powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1
#
# 产物：
#   dist\windows\RongGuang.exe / web\ / bin\    → 绿色免安装版
#   src-tauri\target\release\bundle\nsis\*.exe  → NSIS 安装包（需 tauri-cli）

$ErrorActionPreference = "Stop"

$Root   = Split-Path -Parent $PSScriptRoot
$Web    = Join-Path (Split-Path -Parent $Root) "web"
$Dist   = Join-Path $Root "dist\windows"
$Vendor = Join-Path $Root "vendor\ffmpeg"

Write-Host "==> 构建前端（静态导出）" -ForegroundColor Cyan
Push-Location $Web
pnpm install --prefer-offline
$env:FUSION_DESKTOP_BUILD = "1"
pnpm build
Pop-Location

if (-not (Test-Path (Join-Path $Web "out\index.html"))) {
    throw "前端产物缺失：$Web\out\index.html"
}

Write-Host "==> 编译 Rust（release）" -ForegroundColor Cyan
Push-Location $Root
cargo build --release -p rongguang
Pop-Location

$Bin = Join-Path $Root "target\release\rongguang.exe"
if (-not (Test-Path $Bin)) { throw "未找到 $Bin" }

Write-Host "==> 组装 $Dist" -ForegroundColor Cyan
if (Test-Path $Dist) { Remove-Item -Recurse -Force $Dist }
New-Item -ItemType Directory -Path $Dist, "$Dist\bin" | Out-Null
Copy-Item $Bin $Dist
Copy-Item (Join-Path $Web "out") "$Dist\web" -Recurse

Write-Host "==> 收集 ffmpeg" -ForegroundColor Cyan
# 必须使用静态构建（gyan.dev release-essentials 里的 static 版）。
# 千万别用 C:\Program Files 下某个软件自带的 ffmpeg.exe —— 换台机器就会因缺 DLL 直接崩。
foreach ($tool in @("ffmpeg.exe", "ffprobe.exe")) {
    $candidate = Join-Path $Vendor $tool
    if (Test-Path $candidate) {
        Copy-Item $candidate "$Dist\bin\" -Force
        $mb = [math]::Round((Get-Item $candidate).Length / 1MB)
        Write-Host "    $tool ${mb}MB"
        if ($mb -gt 130) {
            Write-Warning "$tool 体积 ${mb}MB，疑似非静态构建，安装包换机器可能不可用"
        }
    } elseif (Get-Command $tool -ErrorAction SilentlyContinue) {
        Copy-Item (Get-Command $tool).Source "$Dist\bin\" -Force
        Write-Warning "回落到 PATH 中的 $tool，仅供本地自测；发布前请放入 $Vendor"
    } else {
        Write-Warning "缺少 $tool，请放到 $Vendor"
    }
}

Write-Host ""
Write-Host "==> 绿色版完成：$Dist" -ForegroundColor Green
Write-Host "    启动：$Dist\RongGuang.exe"

Write-Host ""
Write-Host "如需安装包，执行（需先安装 tauri-cli）：" -ForegroundColor Yellow
Write-Host "    cargo install tauri-cli --version '^2' --locked"
Write-Host "    cargo tauri build --no-bundle  # 或 cargo tauri build 生成 NSIS"
