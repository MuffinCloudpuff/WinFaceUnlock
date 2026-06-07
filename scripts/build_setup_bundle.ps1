param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..")
$PayloadScript = Join-Path $ScriptDir "build_setup_payload.ps1"
$WinUiScript = Join-Path $ScriptDir "build_setup_winui.ps1"
$PayloadDir = Join-Path $RepoRoot "target\setup-payload"
$BundleRoot = Join-Path $RepoRoot "target\setup-bundle"
$BundleAppDir = Join-Path $BundleRoot "app"
$BundlePayloadDir = Join-Path $BundleRoot "payload"
$WinUiOutputDir = Join-Path $RepoRoot "apps\setup-winui\WinFaceUnlock.Setup.App\bin\x64\$Configuration\net9.0-windows10.0.19041.0\win-x64\publish"
$ControlAppOutputDir = Join-Path $RepoRoot "apps\setup-winui\WinFaceUnlock.Control.App\bin\x64\$Configuration\net9.0-windows10.0.19041.0\win-x64\publish"

& powershell -ExecutionPolicy Bypass -File $WinUiScript -Configuration $Configuration
if ($LASTEXITCODE -ne 0) {
    throw "WinUI setup build failed with exit code $LASTEXITCODE"
}

& powershell -ExecutionPolicy Bypass -File $PayloadScript -ControlAppPublishDir $ControlAppOutputDir
if ($LASTEXITCODE -ne 0) {
    throw "Setup payload build failed with exit code $LASTEXITCODE"
}

if (-not (Test-Path $WinUiOutputDir -PathType Container)) {
    throw "WinUI output directory is missing: $WinUiOutputDir"
}
if (-not (Test-Path $ControlAppOutputDir -PathType Container)) {
    throw "WinUI control output directory is missing: $ControlAppOutputDir"
}

Remove-Item -LiteralPath $BundleRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $BundleAppDir -Force | Out-Null
New-Item -ItemType Directory -Path $BundlePayloadDir -Force | Out-Null

Copy-Item -Path (Join-Path $WinUiOutputDir "*") -Destination $BundleAppDir -Recurse -Force
Copy-Item -Path (Join-Path $PayloadDir "*") -Destination $BundlePayloadDir -Recurse -Force

Write-Host "WinFaceUnlock setup bundle built:"
Write-Host "  bundle_root: $BundleRoot"
Write-Host "  app_dir: $BundleAppDir"
Write-Host "  payload_dir: $BundlePayloadDir"
