param(
    [switch]$NoInstall
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..")
$AppDir = Join-Path $RepoRoot "apps\control-tauri"
$NodeModulesDir = Join-Path $AppDir "node_modules"

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

Start-Process -FilePath $npm -ArgumentList @("run", "tauri:dev") -WorkingDirectory $AppDir
Write-Host "WinFaceUnlock Tauri control app launching:"
Write-Host "  app: $AppDir"
Write-Host "  command: npm run tauri:dev"
