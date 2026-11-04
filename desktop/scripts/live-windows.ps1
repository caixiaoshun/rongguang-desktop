param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$OutputDir,
    [Parameter(Mandatory = $true)][string]$PrivateDir,
    [Parameter(Mandatory = $true)][string]$EvidenceDir
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
$credentials = Get-Content (Join-Path $PrivateDir "credentials.json") -Raw -Encoding UTF8 | ConvertFrom-Json
$secret = [string]$credentials.api_key
$evidence = $EvidenceDir
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
    ($rows | ConvertTo-Json -Depth 5).Replace($secret, "[REDACTED]") | Set-Content (Join-Path $evidence "$stem.json") -Encoding UTF8
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
    [AuditMouse]::SetForegroundWindow($app.MainWindowHandle) | Out-Null
    $controls[0].SetFocus()
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    if ($Text -eq $secret) {
        # Password fields do not expose their value through accessibility. Type
        # the protected value without relying on asynchronous clipboard reads.
        $escaped = [regex]::Replace($Text, '[+^%~(){}\[\]]', { param($match) '{' + $match.Value + '}' })
        [System.Windows.Forms.SendKeys]::SendWait($escaped)
    } else {
        [System.Windows.Forms.Clipboard]::SetText($Text)
        try {
            Start-Sleep -Milliseconds 200
            [System.Windows.Forms.SendKeys]::SendWait('^v')
            Start-Sleep -Milliseconds 600
        } finally { [System.Windows.Forms.Clipboard]::Clear() }
    }
    Start-Sleep -Milliseconds 500
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
        $errorText = $_.ToString().Replace($secret, "[REDACTED]")
        try { Save-State "$Name-failure" | Out-Null } catch {}
        $results.Add([ordered]@{check=$Name;status='failed';error=$errorText;evidence=$script:lastState})
        Write-Warning "$Name`: $errorText"
        # Close a failed dialog/picker so subsequent checks can identify independent failures.
        try { [System.Windows.Forms.SendKeys]::SendWait('{ESC}') } catch {}
    }
    $results | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $evidence 'results.json') -Encoding UTF8
}
function Get-Api([string]$Path) {
    $raw = Invoke-WebRequest ($origin + $Path) -UseBasicParsing -TimeoutSec 35
    $response = [System.Text.Encoding]::UTF8.GetString($raw.RawContentStream.ToArray()) | ConvertFrom-Json
    if ($response.code -ne 0) { throw ([string]$response.msg) }
    return $response.data
}
function Wait-Content([string]$Text, [int]$Seconds = 120) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        $current = Get-Text
        if ($current.Contains($Text)) { return }
        if ($current -match '运行异常|异步任务发生异常|请求失败|连接失败|not allowed by ACL') { throw 'Application reported an error while waiting for content' }
        Start-Sleep -Seconds 2
    } until ([DateTime]::UtcNow -gt $deadline)
    throw "Timed out waiting for content: $Text"
}
function Assistant-Messages {
    $conversations = Get-Api '/api/ai/assistant/conversations?pageNo=1&pageSize=20&category=assistant'
    $conversation = $conversations.list | Sort-Object id -Descending | Select-Object -First 1
    if (-not $conversation) { return @() }
    return @(Get-Api "/api/ai/assistant/conversations/$($conversation.conversationId)/messages" | Where-Object { $_.role -eq 'assistant' })
}
function Wait-AssistantReply([int]$PreviousCount, [string]$Expected) {
    $deadline = [DateTime]::UtcNow.AddSeconds(180)
    do {
        $messages = @(Assistant-Messages)
        if ($messages.Count -gt $PreviousCount) {
            $messages | ConvertTo-Json -Depth 15 | Set-Content (Join-Path $evidence 'assistant-messages.json') -Encoding UTF8
            $last = $messages | Select-Object -Last 1
            if (-not ([string]$last.content).Contains($Expected)) { throw 'Assistant reply did not follow the requested simple instruction' }
            Start-Sleep -Seconds 2
            $visibleReply = @(Get-Elements | Where-Object {
                $name = $_.Current.Name.Trim().TrimEnd([char[]]'。.!！')
                $name -eq $Expected
            })
            if (-not $visibleReply.Count) { throw 'Assistant response was persisted but is not visible in the UI' }
            return
        }
        if ((Get-Text) -match '运行异常|请求失败|调用失败|无效.*密钥') { throw 'Assistant UI reported a request failure' }
        Start-Sleep -Seconds 2
    } until ([DateTime]::UtcNow -gt $deadline)
    throw 'Assistant did not persist a completed response within 180 seconds'
}
function Wait-Started {
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    do {
        Start-Sleep -Milliseconds 500
        $app.Refresh()
        if ($app.HasExited) { throw 'App exited during startup' }
    } until (($app.MainWindowHandle -ne 0 -and $app.MainWindowTitle -eq '融光 · AI 视频创作工作台') -or [DateTime]::UtcNow -gt $deadline)
    [AuditMouse]::ShowWindow($app.MainWindowHandle, 3) | Out-Null
    Start-Sleep -Milliseconds 700
}
function Test-Generation([string]$Kind, [string]$Prompt) {
    $previous = @(Get-Api "/api/generation/$Kind/page?pageNo=1&pageSize=20").list
    $lastId = 0
    if ($previous.Count) { $lastId = ($previous | Measure-Object -Property id -Maximum).Maximum }
    if ($Kind -eq 'image') { Type-Control '图片提示词' $Prompt; Click-Control '生成图片' }
    else { Type-Control '视频提示词' $Prompt; Click-Control '生成视频' }
    Save-State "$Kind-submitted" | Out-Null
    $deadline = [DateTime]::UtcNow.AddSeconds(900)
    $submittedDeadline = [DateTime]::UtcNow.AddSeconds(30)
    $task = $null
    do {
        $page = Get-Api "/api/generation/$Kind/page?pageNo=1&pageSize=20"
        $task = $page.list | Where-Object { $_.id -gt $lastId } | Sort-Object id -Descending | Select-Object -First 1
        if ($task -and $task.status -in @(2,3,4)) { break }
        if (-not $task -and [DateTime]::UtcNow -gt $submittedDeadline) { throw 'UI did not create a task within 30 seconds' }
        Start-Sleep -Seconds 3
    } until ([DateTime]::UtcNow -gt $deadline)
    if (-not $task) { throw 'No generation task was created by the UI' }
    $task | ConvertTo-Json -Depth 12 | Set-Content (Join-Path $evidence "$Kind-task.json") -Encoding UTF8
    if ($Kind -eq 'video' -and $credentials.video_model_15) {
        $model = @(Get-Api '/api/ai/model/list') | Where-Object { $_.id -eq $task.modelId } | Select-Object -First 1
        if ($model.code -ne 'grok-imagine-video-1.5') { throw 'The submitted task did not use the selected Video 1.5 model' }
    }
    if ($task.status -ne 2) { throw "Generation did not succeed: $($task.errorMsg)" }
    $items = @(Get-Api "/api/generation/$Kind/$($task.id)/items")
    $items | ConvertTo-Json -Depth 12 | Set-Content (Join-Path $evidence "$Kind-items.json") -Encoding UTF8
    $media = if ($Kind -eq 'image') { $items[0].imageUrl } else { $items[0].videoUrl }
    if (-not $media) { throw 'Successful task has no usable media URL' }
    $url = if ($media.StartsWith('http')) { $media } else { $origin + $media }
    $mediaDir = Join-Path $evidence 'media'
    New-Item -ItemType Directory -Force $mediaDir | Out-Null
    $fileName = if ($Kind -eq 'image') { 'live-image.png' } else { 'live-video.mp4' }
    $file = Join-Path $mediaDir $fileName
    Invoke-WebRequest $url -OutFile $file -UseBasicParsing -TimeoutSec 60 | Out-Null
    if ((Get-Item $file).Length -lt 1000) { throw 'Media download is empty or invalid' }
    if ($Kind -eq 'image') {
        $image = [System.Drawing.Image]::FromFile($file)
        try { @{width=$image.Width;height=$image.Height} | ConvertTo-Json | Set-Content (Join-Path $evidence 'image-dimensions.json') -Encoding UTF8 }
        finally { $image.Dispose() }
    } else {
        & (Join-Path $installDir 'bin/ffprobe.exe') -v error -show_entries 'format=duration:stream=codec_type,width,height' -of json $file | Set-Content (Join-Path $evidence 'video-probe.json') -Encoding UTF8
        if ($LASTEXITCODE -ne 0) { throw 'Video could not be decoded by ffprobe' }
    }
    Click-Control '刷新记录'
    if ((Get-Text) -match '生成失败') { throw 'UI reports failure for a successfully persisted generation' }
}
try {
    Wait-Started
    $port = (Get-Content (Join-Path $env:FUSION_DATA_DIR 'runtime-port') -Raw).Trim()
    $origin = "http://127.0.0.1:$port"
    Check 'initial' {} '最近项目'
    Check 'open-ai-config' { Click-Control '系统设置' 'header'; Click-Control 'AI 配置' 'sidebar' } '添加 API 配置'
    Check 'configure-grok-through-ui' {
        Click-Control '编辑 API 配置'
        Type-Control 'https://你的域名（只填根地址，不要带 /v1）' ([string]$credentials.base_url)
        Type-Control '你的 Grok 服务密钥' $secret
        Click-Control '保存'
        $configs = @(Get-Api '/api/ai/api-config/list')
        $config = $configs | Where-Object { $_.platform -eq 'grok' } | Select-Object -First 1
        if ($config.apiKey -ne $secret -or $config.apiUrl.TrimEnd('/') -ne ([string]$credentials.base_url).TrimEnd('/')) { throw 'Configuration was not persisted correctly' }
    } '添加 API 配置'
    if ($results[$results.Count - 1].status -ne 'passed') { throw 'Live tests blocked because UI configuration did not succeed' }
    Check 'fetch-models-through-ui' {
        Click-Control '获取可用模型列表'
        $discovery = Get-Content (Join-Path $evidence 'provider-models.json') -Raw -Encoding UTF8 | ConvertFrom-Json
        Wait-Content ([string]$discovery.models[0]) 80
    } '获取可用模型'
    Check 'close-models-dialog' { [System.Windows.Forms.SendKeys]::SendWait('{ESC}') } '添加 API 配置'
    Check 'assistant-chat' {
        Click-Control '仪表盘' 'header'
        Click-Control '打开融光助手'
        $previousCount = @(Assistant-Messages).Count
        Type-Control '助手消息；输入 @ 引用项目，输入 / 引用 Skill 或 MCP' '请只回复：连接验证成功。不要调用工具。'
        Click-Control '发送'
        Wait-AssistantReply $previousCount '连接验证成功'
    } '连接验证成功'
    Check 'assistant-multi-turn' {
        $previousCount = @(Assistant-Messages).Count
        Type-Control '助手消息；输入 @ 引用项目，输入 / 引用 Skill 或 MCP' '在你的上一条回复后加上两个汉字“复测”，只输出拼接后的文字，去掉标点。'
        Click-Control '发送'
        Wait-AssistantReply $previousCount '连接验证成功复测'
    } '连接验证成功复测'
    Check 'assistant-close' { Click-Control '收起助手' } '打开融光助手'
    if ($credentials.video_model_15) {
        Check 'open-image-workbench' { Click-Control '工具' 'header' } '生图'
    } else {
        Check 'image-workbench-live' {
            Click-Control '工具' 'header'
            Test-Generation 'image' 'RGA-LIVE-IMAGE. Cinematic storyboard frame, an adult Chinese female archivist with short black hair and a beige coat stands in a quiet futuristic archive at night. A small amber memory cube glows on the desk. Restrained teal and amber lighting, realistic film still, wide shot, 16:9, no text.'
        }
    }
    Check 'video-workbench-live' {
        Click-Control '生视频' 'sidebar'
        if ($credentials.video_model_15) {
            Click-Control '选择生成模型'
            Click-Control 'Grok Imagine 视频 1.5 grok-imagine-video-1.5'
        }
        Test-Generation 'video' 'RGA-LIVE-VIDEO. Cinematic live action wide shot inside a quiet futuristic archive at night. An adult Chinese female archivist with short black hair and a beige coat gently picks up a glowing amber memory cube. Slow dolly in, subtle natural movement, teal and amber lighting, no captions, no logos.'
    }
    Check 'restart-preserves-settings-and-history' {
        $app.CloseMainWindow() | Out-Null
        if (-not $app.WaitForExit(12000)) { throw 'App did not exit normally' }
        $script:app = Start-Process (Join-Path $installDir 'rongguang.exe') -WorkingDirectory $installDir -PassThru
        Wait-Started
        $script:port = (Get-Content (Join-Path $env:FUSION_DATA_DIR 'runtime-port') -Raw).Trim()
        $script:origin = "http://127.0.0.1:$port"
        $config = @(Get-Api '/api/ai/api-config/list') | Where-Object { $_.platform -eq 'grok' } | Select-Object -First 1
        if ($config.apiKey -ne $secret) { throw 'API configuration did not survive restart' }
        $conversations = Get-Api '/api/ai/assistant/conversations?pageNo=1&pageSize=20&category=assistant'
        if (-not $conversations.list.Count) { throw 'No conversation history after real chat' }
    } '最近项目'
} finally {
    try {
        Get-Api '/api/ai/assistant/conversations?pageNo=1&pageSize=20' | ConvertTo-Json -Depth 15 | Set-Content (Join-Path $evidence 'conversations.json') -Encoding UTF8
        Get-Api '/api/ai/pipeline/running' | ConvertTo-Json -Depth 15 | Set-Content (Join-Path $evidence 'running-pipelines.json') -Encoding UTF8
        if (Test-Path (Join-Path $env:FUSION_DATA_DIR 'logs')) { Copy-Item (Join-Path $env:FUSION_DATA_DIR 'logs') (Join-Path $evidence 'logs') -Recurse -Force }
    } catch { Write-Warning 'Could not capture final backend diagnostics' }
    $results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $evidence 'results.json') -Encoding UTF8
    if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force }
    [System.Windows.Forms.Clipboard]::Clear()
}
$failed = @($results | Where-Object { $_.status -eq 'failed' })
Write-Host "Live interaction results: $($results.Count) checks, $($failed.Count) failures"
if ($failed.Count) { exit 1 }
