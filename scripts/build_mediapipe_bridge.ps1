param(
    [Parameter(Mandatory = $true)]
    [string] $MediaPipeIncludeDir,

    [Parameter(Mandatory = $true)]
    [string] $MediaPipeTasksCLib,

    [string] $BuildDir = "target\mediapipe_bridge",

    [string] $Configuration = "Release"
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$sourceDir = Join-Path $repoRoot "native\mediapipe_bridge"
$absoluteBuildDir = Join-Path $repoRoot $BuildDir
$outputDir = Join-Path $repoRoot "native"
$outputDll = Join-Path $outputDir "winfaceunlock_mediapipe_bridge.dll"

if (-not (Test-Path $MediaPipeIncludeDir)) {
    throw "MediaPipe include directory not found: $MediaPipeIncludeDir"
}

if (-not (Test-Path $MediaPipeTasksCLib)) {
    throw "MediaPipe Tasks C library not found: $MediaPipeTasksCLib"
}

cmake `
    -S $sourceDir `
    -B $absoluteBuildDir `
    -G Ninja `
    -DCMAKE_BUILD_TYPE=$Configuration `
    -DMEDIAPIPE_INCLUDE_DIR=$MediaPipeIncludeDir `
    -DMEDIAPIPE_TASKS_C_LIB=$MediaPipeTasksCLib

cmake --build $absoluteBuildDir --config $Configuration

$builtDll = Get-ChildItem -Path $absoluteBuildDir -Recurse -Filter "winfaceunlock_mediapipe_bridge.dll" |
    Select-Object -First 1

if ($null -eq $builtDll) {
    throw "Built bridge DLL was not found under $absoluteBuildDir"
}

New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
Copy-Item -Path $builtDll.FullName -Destination $outputDll -Force

Write-Output "bridge_dll: $outputDll"
