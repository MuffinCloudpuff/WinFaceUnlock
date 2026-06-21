param(
    [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..")
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $RepoRoot "models\pose_landmarker_lite.task"
}

$OutputPath = [System.IO.Path]::GetFullPath($OutputPath)
$OutputDir = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

$Url = "https://storage.googleapis.com/mediapipe-models/pose_landmarker/pose_landmarker_lite/float16/latest/pose_landmarker_lite.task"
Invoke-WebRequest -Uri $Url -OutFile $OutputPath

if (-not (Test-Path $OutputPath -PathType Leaf)) {
    throw "MediaPipe Pose Lite model download failed: $OutputPath"
}

$Length = (Get-Item -LiteralPath $OutputPath).Length
if ($Length -lt 1000000) {
    throw "MediaPipe Pose Lite model is unexpectedly small: $Length bytes"
}

Write-Host "mediapipe_pose_lite_model: $OutputPath"
