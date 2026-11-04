param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$OutputDir
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class AuditMouse {
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr handle, int command);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr handle);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
}
'@
$evidence = Join-Path $OutputDir "evidence"
New-Item -ItemType Directory -Force $evidence | Out-Null
$installDir = Join-Path $OutputDir "install"
$setup = Start-Process -FilePath $Installer -ArgumentList @('/S', "/D=$installDir") -PassThru -Wait
if ($setup.ExitCode -ne 0) { throw "Installer failed" }
$env:FUSION_DATA_DIR = Join-Path $OutputDir "data"
$env:FUSION_WEB_DIR = Join-Path $installDir "web"
$env:FUSION_PORT = "0"
$app = Start-Process (Join-Path $installDir 'rongguang.exe') -WorkingDirectory $installDir -PassThru
$results = [System.Collections.Generic.List[object]]::new()
$script:sequence = 0
$script:lastState = ""

function Get-Elements {
    $app.Refresh()
    if ($app.HasExited -or -not $app.Responding) { throw "Application exited or stopped responding" }
    $script:root = [System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
    return $script:root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
}
function Get-Text {
    return ((Get-Elements | ForEach-Object { try { $_.Current.Name } catch {} }) -join "`n")
}
function Save-State([string]$Label) {
    $script:sequence++
    $stem = '{0:D3}-{1}' -f $script:sequence, ($Label -replace '[^a-zA-Z0-9_-]', '_')
    $script:lastState = $stem
    $rows = @(Get-Elements | ForEach-Object {
        try { [ordered]@{ name=$_.Current.Name; type=$_.Current.ControlType.ProgrammaticName; enabled=$_.Current.IsEnabled; offscreen=$_.Current.IsOffscreen; id=$_.Current.AutomationId } } catch {}
    })
    $rows | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $evidence "$stem.json") -Encoding UTF8
    try {
        $area = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $bmp = [System.Drawing.Bitmap]::new($area.Width, $area.Height)
        $graphics = [System.Drawing.Graphics]::FromImage($bmp)
        try { $graphics.CopyFromScreen($area.Location, [System.Drawing.Point]::Empty, $area.Size); $bmp.Save((Join-Path $evidence "$stem.png"), [System.Drawing.Imaging.ImageFormat]::Png) }
        finally { $graphics.Dispose(); $bmp.Dispose() }
    } catch { Write-Warning $_ }
    return ($rows.name -join "`n")
}
function Click-Control([string]$Name, [string]$Region = 'any', [string]$Method = 'mouse') {
    $key = $Name -replace '\s', ''
    $controlDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
    $matches = @(Get-Elements | Where-Object {
        try {
            $n = $_.Current.Name -replace '\s', ''
            $b = $_.Current.BoundingRectangle
            $_.Current.IsEnabled -and ($n -eq $key -or $n -eq "$key$key") -and
            ($Region -ne 'header' -or $b.Top -lt 120) -and
            ($Region -ne 'sidebar' -or ($b.Top -gt 110 -and $b.Left -lt 320))
        } catch { $false }
    })
    if ($matches.Count -gt 0) { break }
    Start-Sleep -Milliseconds 300
    } until ([DateTime]::UtcNow -gt $controlDeadline)
    $visibleMatches = @($matches | Where-Object { -not $_.Current.IsOffscreen })
    if ($visibleMatches.Count) { $matches = $visibleMatches }
    $controls = @($matches | Where-Object { $_.Current.ControlType.ProgrammaticName -match 'Button|Hyperlink|TabItem|MenuItem|RadioButton' })
    if ($controls.Count -gt 0) { $matches = $controls }
    if ($matches.Count -eq 0) { throw "Control not found: $Name ($Region)" }
    if ($controls.Count -gt 1) { throw "Ambiguous control: $Name ($Region), $($controls.Count) matches" }
    $visible = @($matches | Where-Object { -not $_.Current.IsOffscreen })
    $element = if ($visible.Count) { $visible[0] } else { $matches[0] }
    if ($element.Current.IsOffscreen) {
        $scroll = $null
        if ($element.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern, [ref]$scroll)) {
            ([System.Windows.Automation.ScrollItemPattern]$scroll).ScrollIntoView()
        } else {
            $element.SetFocus()
        }
        Start-Sleep -Milliseconds 400
        if ($element.Current.IsOffscreen) { throw "Could not scroll control into view: $Name" }
    }
    $point = [System.Windows.Point]::new(0, 0)
    $hasPoint = $element.TryGetClickablePoint([ref]$point)
    $screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $pattern = $null
    if ($Method -eq 'keyboard') {
        [AuditMouse]::SetForegroundWindow($app.MainWindowHandle) | Out-Null
        $element.SetFocus()
        [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
    } elseif ($Method -eq 'accessibility') {
        if (-not $element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) { throw "No accessibility activation for $Name" }
        ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
    } elseif ($hasPoint -and $screen.Contains([int]$point.X, [int]$point.Y)) {
        [AuditMouse]::SetForegroundWindow($app.MainWindowHandle) | Out-Null
        [AuditMouse]::SetCursorPos([int]$point.X, [int]$point.Y) | Out-Null
        [AuditMouse]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
        [AuditMouse]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    } elseif ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
    } elseif ($element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
        ([System.Windows.Automation.SelectionItemPattern]$pattern).Select()
    } else {
        $point = $element.GetClickablePoint()
        [AuditMouse]::SetCursorPos([int]$point.X, [int]$point.Y) | Out-Null
        [AuditMouse]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
        [AuditMouse]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    }
    Start-Sleep -Milliseconds 1200
}
function Type-Control([string]$Name, [string]$Text) {
    $controls = @(Get-Elements | Where-Object {
        $_.Current.ControlType.ProgrammaticName -eq 'ControlType.Edit' -and
        ($_.Current.Name -eq $Name -or $_.Current.AutomationId -eq $Name)
    })
    if ($controls.Count -ne 1) { throw "Expected one input: $Name, found $($controls.Count)" }
    $controls[0].SetFocus()
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    [System.Windows.Forms.SendKeys]::SendWait($Text)
    Start-Sleep -Milliseconds 600
}
function Open-External([string]$Name, [string]$Method = 'mouse') {
    $before = @(Get-ChildItem (Join-Path $env:FUSION_DATA_DIR 'logs') -File | Select-String 'external_url_opened').Count
    Click-Control $Name 'any' $Method
    $immediate = Save-State ("external-" + $Name)
    if ($immediate -match '运行异常|异步任务发生异常|not allowed|无法打开链接|forbidden') { throw "External link error: $immediate" }
    Start-Sleep -Seconds 2
    $after = @(Get-ChildItem (Join-Path $env:FUSION_DATA_DIR 'logs') -File | Select-String 'external_url_opened').Count
    if ($after -le $before) { throw "Native browser opener did not report success: $Name" }
    [AuditMouse]::SetForegroundWindow($app.MainWindowHandle) | Out-Null
}
function Check([string]$Name, [scriptblock]$Action, [string[]]$Expected = @()) {
    try {
        @{check=$Name;status='running'} | ConvertTo-Json | Set-Content (Join-Path $evidence 'current-check.json') -Encoding UTF8
        & $Action
        if ($Expected) {
            $contentDeadline = [DateTime]::UtcNow.AddSeconds(15)
            do {
                $currentText = Get-Text
                $missing = @($Expected | Where-Object { -not $currentText.Contains($_) })
                if ($missing.Count -eq 0) { break }
                Start-Sleep -Milliseconds 300
            } until ([DateTime]::UtcNow -gt $contentDeadline)
        }
        $text = Save-State $Name
        $missing = @($Expected | Where-Object { -not $text.Contains($_) })
        if ($missing.Count -gt 0) { throw "Expected content missing: $($missing -join ', ')" }
        if ($text -match 'Application error|运行异常|异步任务发生异常|Cannot read properties|not allowed by ACL|无法打开链接|顶部导航加载失败|请求失败|加载失败|发生了意外错误') { throw "UI reported an error: $($Matches[0])" }
        $results.Add([ordered]@{check=$Name;status='passed';evidence=$script:lastState})
        Write-Host "PASS: $Name"
    } catch {
        $errorText = $_.ToString()
        try { Save-State "$Name-failure" | Out-Null } catch {}
        $results.Add([ordered]@{check=$Name;status='failed';error=$errorText;evidence=$script:lastState})
        Write-Warning "$Name`: $errorText"
        # Close a failed dialog/picker so subsequent checks can identify independent failures.
        try { [System.Windows.Forms.SendKeys]::SendWait('{ESC}') } catch {}
    }
    $results | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $evidence 'results.json') -Encoding UTF8
}
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    do {
        Start-Sleep -Milliseconds 500
        $app.Refresh()
        if ($app.HasExited) { throw "App exited during startup" }
    } until (($app.MainWindowHandle -ne 0 -and $app.MainWindowTitle -eq '融光 · AI 视频创作工作台') -or [DateTime]::UtcNow -gt $deadline)
    # The runner desktop can be smaller than the default 1440x900 window.
    # Maximize before clicking so controls cannot sit behind the taskbar.
    [AuditMouse]::ShowWindow($app.MainWindowHandle, 3) | Out-Null
    Start-Sleep -Milliseconds 700
    $port = (Get-Content (Join-Path $env:FUSION_DATA_DIR 'runtime-port') -Raw).Trim()
    $origin = "http://127.0.0.1:$port"
    Check 'initial' {} '最近项目'
    Check 'theme-light' { Click-Control '切换主题' }
    Check 'theme-dark' { Click-Control '切换主题' }
    Check 'task-center-open' { Click-Control '打开任务中心' } 'AI 任务中心'
    Check 'task-center-close' { Click-Control '关闭任务中心' }
    Check 'user-menu-open' { Click-Control '打开用户菜单' } '个人设置'
    Check 'user-menu-settings' { Click-Control '个人设置' } '个人设置'
    Check 'back-dashboard' { Click-Control '仪表盘' 'header' } '最近项目'
    Check 'analytics' { Click-Control '数据分析' 'sidebar' } '数据分析'
    Check 'dashboard' { Click-Control '总览' 'sidebar' } '最近项目'
    Check 'projects' { Click-Control '项目' 'header' } '项目管理'
    Check 'create-project-dialog' { Click-Control '新建项目' } '创建项目'
    Check 'cancel-project-dialog' { Click-Control '取消' } '项目管理'
    # Enter and submit a real form, using only this clean CI data directory.
    Check 'create-project-submit' {
        Click-Control '新建项目'
        Type-Control '例如：品牌宣传片' 'UI-AUDIT-Test'
        Type-Control '简要描述项目内容和目标...' 'Disposable interaction audit project'
        Click-Control '创建项目'
    } 'UI-AUDIT-Test'
    Check 'reload-dashboard' { Click-Control '仪表盘' 'header' } '最近项目'
    Check 'reload-projects' { Click-Control '项目' 'header' } 'UI-AUDIT-Test'
    Check 'project-overview' { Click-Control 'UI-AUDIT-Test' } '剧本'
    Check 'project-scripts' { Click-Control '剧本' 'sidebar' } '剧本'
    Check 'project-storyboards' { Click-Control '分镜' 'sidebar' } '分镜'
    Check 'project-assets' { Click-Control '资产' 'sidebar' } '资产'
    Check 'project-settings' { Click-Control '设置' 'sidebar' } @('项目设置', '项目名称', '保存设置')
    Check 'project-settings-save' {
        Type-Control 'project-name' 'UI-AUDIT-Renamed'
        Click-Control '保存设置'
    } '项目设置已保存'
    Check 'project-settings-restore-name' {
        Type-Control 'project-name' 'UI-AUDIT-Test'
        Click-Control '保存设置'
    } '项目设置已保存'
    Check 'project-delete-dialog' { Click-Control '删除项目' 'any' 'accessibility' } '彻底删除项目'
    Check 'project-delete-cancel' { Click-Control '取消' } '项目设置'
    Check 'global-assets'  { Click-Control '资产' 'header' } '素材资产'
    Check 'image-workbench' { Click-Control '工具' 'header' } '生图'
    Check 'video-workbench' { Click-Control '生视频' 'sidebar' } '生视频'
    Check 'settings' { Click-Control '系统设置' 'header' } '管理系统全局参数'
    $settingsPages = [ordered]@{
        '通用设置' = '管理系统全局参数'
        '用户列表' = '用户总数'
        '个人设置' = '个人信息'
        'AI 配置' = '添加 API 配置'
        '智能体配置' = '我的 Skills'
        '存储配置' = '添加存储配置'
    }
    $settingsIndex = 0
    foreach ($name in $settingsPages.Keys) {
        $label = $name
        Check ("settings-" + $settingsIndex) { Click-Control $label 'sidebar' } $settingsPages[$name]
        $settingsIndex++
    }
    Check 'agent-settings-dialogs' { Click-Control '智能体配置' 'sidebar' } '我的 Skills'
    Check 'skill-add' { Click-Control '新建 Skill' } 'SKILL.md 正文'
    Check 'skill-cancel' { Click-Control '取消' } '我的 Skills'
    Check 'skill-create' {
        Click-Control '新建 Skill'
        Type-Control 'skill-display-name' 'UI Audit Skill'
        Type-Control 'skill-name' 'ui-audit'
        Type-Control 'skill-description' 'Disposable UI audit skill'
        Type-Control 'skill-content' 'Summarize the current project.'
        Click-Control '保存'
    } 'UI Audit Skill'
    Check 'skill-edit' { Click-Control '编辑 ui-audit' } '编辑 Skill'
    Check 'skill-edit-cancel' { Click-Control '取消' } 'UI Audit Skill'
    Check 'mcp-add' { Click-Control '添加 MCP' } '添加 MCP 服务'
    Check 'mcp-cancel' { Click-Control '取消' } '我的 Skills'
    Check 'general-settings-release' { Click-Control '通用设置' 'sidebar' } '管理系统全局参数'
    Check 'release-link' { Open-External '打开 Release 页面' }
    Check 'back-storage' { Click-Control '存储配置' 'sidebar' } '添加存储配置'
    Check 'storage-add' { Click-Control '添加存储配置' } '存储'
    Check 'storage-cancel' { Click-Control '取消' } '存储配置'
    Check 'ai-settings' { Click-Control 'AI 配置' 'sidebar' } 'AI 配置'
    Check 'api-add' { Click-Control '添加 API 配置' } 'API'
    Check 'api-cancel' { Click-Control '取消' } 'AI 配置'
    Check 'assistant-home' { Click-Control '仪表盘' 'header' } '最近项目'
    Check 'assistant-open' { Click-Control '打开融光助手' } '新对话'
    Check 'assistant-history' { Click-Control '打开会话列表' } '还没有助手会话'
    Check 'assistant-history-close' { [System.Windows.Forms.SendKeys]::SendWait('{ESC}'); Start-Sleep -Milliseconds 500 }
    Check 'assistant-project-picker' { Type-Control '助手消息；输入 @ 引用项目，输入 / 引用 Skill 或 MCP' '@' } @('引用项目', 'UI-AUDIT-Test')
    Check 'assistant-skill-picker' { Type-Control '助手消息；输入 @ 引用项目，输入 / 引用 Skill 或 MCP' '/' } @('引用 Skill 或 MCP', 'UI Audit Skill')
    Check 'assistant-clear-input' { Type-Control '助手消息；输入 @ 引用项目，输入 / 引用 Skill 或 MCP' '{BACKSPACE}' }
    Check 'assistant-maximize' { Click-Control '最大化' } '恢复窗口'
    Check 'assistant-restore' { Click-Control '恢复窗口' } '最大化'
    Check 'assistant-close' { Click-Control '收起助手' } '打开融光助手'
    Check 'assistant-reopen' { Click-Control '打开融光助手' } '新对话'
    Check 'assistant-close-again' { Click-Control '收起助手' } '打开融光助手'
    Check 'github-link' { Open-External '打开 GitHub 仓库' }
    Check 'github-link-keyboard' { Open-External '打开 GitHub 仓库' 'keyboard' }
    Check 'github-link-accessibility' { Open-External '打开 GitHub 仓库' 'accessibility' }
} finally {
    $results | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $evidence 'results.json') -Encoding UTF8
    if (Test-Path (Join-Path $env:FUSION_DATA_DIR 'logs')) { Copy-Item (Join-Path $env:FUSION_DATA_DIR 'logs') (Join-Path $evidence 'logs') -Recurse -Force }
    if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force }
}
$failed = @($results | Where-Object { $_.status -eq 'failed' })
Write-Host "Interaction results: $($results.Count) checks, $($failed.Count) failures"
if ($failed.Count -gt 0) { exit 1 }
