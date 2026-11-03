# Run under Windows PowerShell for the built-in UI Automation assemblies.
# Keep this file ASCII so Windows PowerShell 5.1 reads it consistently.
param(
    [Parameter(Mandatory = $true)][long]$WindowHandle,
    [Parameter(Mandatory = $true)][ValidateSet("normal", "error")][string]$Mode,
    [Parameter(Mandatory = $true)][string]$Screenshot
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

$window = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]::new($WindowHandle))
$deadline = [DateTime]::UtcNow.AddSeconds(20)
$text = ""
while ([DateTime]::UtcNow -lt $deadline) {
    $elements = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    $names = foreach ($element in $elements) {
        try { $element.Current.Name } catch { }
    }
    $text = $names -join "`n"
    $project = [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String("6aG555uu"))
    $asset = [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String("57Sg5p2Q"))
    $valid = if ($Mode -eq "error") { $text.Contains("10048") } else { $text.Contains($project) -and $text.Contains($asset) }
    if ($valid) { break }
    Start-Sleep -Milliseconds 500
}

# Preserve the actual runner desktop for visual inspection of the installed app.
try {
    $area = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = [System.Drawing.Bitmap]::new($area.Width, $area.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($area.Location, [System.Drawing.Point]::Empty, $area.Size)
        $bitmap.Save($Screenshot, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
} catch { Write-Warning "Screenshot capture unavailable: $_" }

if (-not $valid) {
    Write-Host ("Visible accessibility text: " + $text)
    throw "Expected window content not found: $Mode"
}
Write-Host "PASS: native Windows accessibility content $Mode ($($text.Length) characters)"
