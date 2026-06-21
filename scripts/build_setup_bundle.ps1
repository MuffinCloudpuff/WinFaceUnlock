param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [switch]$SkipWinUiBuild,
    [switch]$SkipTauriBuild
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
$ControlTauriDir = Join-Path $RepoRoot "apps\control-tauri"
$ControlTauriOutputDir = Join-Path $ControlTauriDir "src-tauri\target\x86_64-pc-windows-msvc\$($Configuration.ToLowerInvariant())"
$ControlAppOutputDir = Join-Path $RepoRoot "target\control-tauri-package"

if (-not $SkipWinUiBuild) {
    & powershell -ExecutionPolicy Bypass -File $WinUiScript -Configuration $Configuration
    if ($LASTEXITCODE -ne 0) {
        throw "WinUI setup build failed with exit code $LASTEXITCODE"
    }
}
elseif (-not (Test-Path $WinUiOutputDir -PathType Container)) {
    throw "Cannot skip WinUI setup build because output directory is missing: $WinUiOutputDir"
}

if (-not $SkipTauriBuild) {
    Push-Location $ControlTauriDir
    try {
        & npm.cmd run tauri:build -- --target x86_64-pc-windows-msvc --no-bundle
        if ($LASTEXITCODE -ne 0) {
            throw "Tauri control app build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

if (-not (Test-Path (Join-Path $ControlTauriOutputDir "WinFaceUnlock.exe") -PathType Leaf)) {
    throw "Tauri control executable is missing: $ControlTauriOutputDir\WinFaceUnlock.exe"
}
Remove-Item -LiteralPath $ControlAppOutputDir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $ControlAppOutputDir -Force | Out-Null
Get-ChildItem -LiteralPath $ControlTauriOutputDir -File | Where-Object {
    $_.Extension -in @(".exe", ".dll")
} | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $ControlAppOutputDir -Force
}

& powershell -ExecutionPolicy Bypass -File $PayloadScript -ControlAppPublishDir $ControlAppOutputDir
if ($LASTEXITCODE -ne 0) {
    throw "Setup payload build failed with exit code $LASTEXITCODE"
}

if (-not (Test-Path $WinUiOutputDir -PathType Container)) {
    throw "WinUI output directory is missing: $WinUiOutputDir"
}
if (-not (Test-Path $ControlAppOutputDir -PathType Container)) {
    throw "Tauri control output directory is missing: $ControlAppOutputDir"
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
