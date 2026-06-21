param(
    [string]$InstallRoot = "C:\WinFaceUnlock",
    [string]$WindowsUserName = $env:USERNAME,
    [ValidateSet("local", "microsoft", "domain")]
    [string]$AccountType = "local",
    [string]$CameraId = "opencv-index:0",
    [switch]$SkipCredentialEnrollment,
    [switch]$InstallProvider,
    [switch]$ShowTileBeforeReady,
    [switch]$NoAutoWakeOnAdvise,
    [switch]$ManualTestProvider
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Assert-Admin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Run this script from an elevated PowerShell session."
    }
}

function Assert-File {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Required file is missing: $Path"
    }
}

Assert-Admin

$diagnostics = Join-Path $InstallRoot "diagnostics_cli.exe"
$installer = Join-Path $InstallRoot "installer_cli.exe"
$service = Join-Path $InstallRoot "win_service.exe"
$provider = Join-Path $InstallRoot "provider\windows_provider.dll"
$template = Join-Path $InstallRoot "phase4-face-template.json"
$models = Join-Path $InstallRoot "models"
$yunet = Join-Path $models "face_detection_yunet_2023mar.onnx"
$sface = Join-Path $models "face_recognition_sface_2021dec.onnx"
$minifasnet = Join-Path $models "minifasnet_v2.onnx"

Assert-File $diagnostics
Assert-File $installer
Assert-File $service
Assert-File $provider
Assert-File $template
Assert-File $yunet
Assert-File $sface
Assert-File $minifasnet

Set-Location $InstallRoot
$logDir = Join-Path $InstallRoot "vm-validation-logs"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$transcript = Join-Path $logDir ("validation-" + (Get-Date -Format "yyyyMMdd-HHmmss") + ".log")
Start-Transcript -Path $transcript | Out-Null

try {
    Write-Host "== Camera discovery =="
    & $diagnostics list-cameras
    & $diagnostics test-camera --camera-id $CameraId --frame-width 640 --frame-height 480

    if (-not $SkipCredentialEnrollment) {
        Write-Host "== Enroll Windows credential =="
        Write-Host "The next command prompts for the Windows password twice. It must not be passed on the command line."
        & $diagnostics enroll-windows-credential --username $WindowsUserName --user-id dev-user --account-type $AccountType
    }

    Write-Host "== Configure local-camera Service auth =="
    & $installer configure-service-auth `
        --face-template $template `
        --camera-id $CameraId `
        --yunet-model $yunet `
        --sface-model $sface `
        --minifasnet-model $minifasnet `
        --minifasnet-crop-scale 2.7 `
        --minifasnet-max-spoof-frame-ratio 0.40 `
        --match-threshold 0.75 `
        --required-consecutive 2
    & $installer service-auth-status

    Write-Host "== Install or repair Service =="
    & $installer repair-service --service-binary $service
    & $installer start-service
    & $installer service-status
    & $diagnostics health-check

    Write-Host "== Service local-camera authentication =="
    & $diagnostics service-camera-auth --session-id vm-face-flow

    if ($InstallProvider) {
        Write-Host "== Install Credential Provider =="
        $providerArgs = @("install-provider", "--provider-binary", $provider)
        if ($ManualTestProvider) {
            $providerArgs += @("--wake-source", "manual-test")
        } else {
            $providerArgs += @("--wake-source", "local-camera")
        }
        if ($ShowTileBeforeReady) {
            $providerArgs += "--show-tile-before-ready"
        }
        if ($NoAutoWakeOnAdvise) {
            $providerArgs += "--no-auto-wake-on-advise"
        }
        & $installer @providerArgs
        & $installer provider-status
    }

    Write-Host "== Validation complete =="
    Write-Host "Log: $transcript"
    Write-Host "Cleanup commands:"
    Write-Host "  .\installer_cli.exe uninstall-provider"
    Write-Host "  .\installer_cli.exe emergency-disable-provider"
    Write-Host "  .\installer_cli.exe stop-service"
    Write-Host "  .\installer_cli.exe uninstall-service"
}
finally {
    Stop-Transcript | Out-Null
}
