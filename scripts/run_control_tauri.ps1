param(
    [switch]$NoInstall
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..")
$AppDir = Join-Path $RepoRoot "apps\control-tauri"
$NodeModulesDir = Join-Path $AppDir "node_modules"
$DiagnosticsCli = Join-Path $RepoRoot "target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe"
$EnrollmentOutputDir = Join-Path $RepoRoot "target\local-control-face-enrollment"
$YunetModelPath = Join-Path $RepoRoot "models\face_detection_yunet_2023mar.onnx"
$SFaceModelPath = Join-Path $RepoRoot "models\face_recognition_sface_2021dec.onnx"

if (-not (Test-Path $AppDir -PathType Container)) {
    throw "Tauri control app directory is missing: $AppDir"
}

$npm = (Get-Command npm.cmd -ErrorAction Stop).Source

if (-not $NoInstall -and -not (Test-Path $NodeModulesDir -PathType Container)) {
    Push-Location $AppDir
    try {
        & $npm install
        if ($LASTEXITCODE -ne 0) {
            throw "npm install failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

$cargo = (Get-Command cargo.exe -ErrorAction Stop).Source
Push-Location $RepoRoot
try {
    & $cargo build -p diagnostics_cli
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build -p diagnostics_cli failed with exit code $LASTEXITCODE"
    }
}
finally {
    Pop-Location
}

foreach ($modelPath in @($YunetModelPath, $SFaceModelPath)) {
    if (-not (Test-Path $modelPath -PathType Leaf)) {
        throw "Required face model is missing: $modelPath"
    }
}

New-Item -ItemType Directory -Force -Path $EnrollmentOutputDir | Out-Null

$env:WINFACEUNLOCK_DIAGNOSTICS_CLI = $DiagnosticsCli
$env:WINFACEUNLOCK_FACE_ENROLLMENT_OUTPUT_DIR = $EnrollmentOutputDir
$env:WINFACEUNLOCK_YUNET_MODEL_PATH = $YunetModelPath
$env:WINFACEUNLOCK_SFACE_MODEL_PATH = $SFaceModelPath

Start-Process -FilePath $npm -ArgumentList @("run", "tauri:dev") -WorkingDirectory $AppDir
Write-Host "WinFaceUnlock Tauri control app launching:"
Write-Host "  app: $AppDir"
Write-Host "  command: npm run tauri:dev"
Write-Host "  diagnostics: $DiagnosticsCli"
Write-Host "  enrollment output: $EnrollmentOutputDir"
