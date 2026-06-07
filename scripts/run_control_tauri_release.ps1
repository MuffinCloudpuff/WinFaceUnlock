param(
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..")
$AppDir = Join-Path $RepoRoot "apps\control-tauri"
$ReleaseExe = Join-Path $AppDir "src-tauri\target\x86_64-pc-windows-msvc\release\WinFaceUnlock.exe"

if (-not (Test-Path $AppDir -PathType Container)) {
    throw "Tauri control app directory is missing: $AppDir"
}

if (-not $NoBuild) {
    $npm = (Get-Command npm.cmd -ErrorAction Stop).Source

    Push-Location $AppDir
    try {
        & $npm install
        if ($LASTEXITCODE -ne 0) {
            throw "npm install failed with exit code $LASTEXITCODE"
        }

        & $npm run tauri:build -- --target x86_64-pc-windows-msvc --no-bundle
        if ($LASTEXITCODE -ne 0) {
            throw "Tauri release build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

if (-not (Test-Path $ReleaseExe -PathType Leaf)) {
    throw "WinFaceUnlock Tauri release executable is missing: $ReleaseExe"
}

Start-Process -FilePath $ReleaseExe -WorkingDirectory (Split-Path $ReleaseExe)
Write-Host "WinFaceUnlock Tauri release app launched:"
Write-Host "  exe: $ReleaseExe"
