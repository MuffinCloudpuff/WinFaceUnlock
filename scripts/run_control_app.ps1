param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Debug",
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..")
$ProjectPath = Join-Path $RepoRoot "apps\setup-winui\WinFaceUnlock.Control.App\WinFaceUnlock.Control.App.csproj"
$PublishDir = Join-Path $RepoRoot "apps\setup-winui\WinFaceUnlock.Control.App\bin\x64\$Configuration\net9.0-windows10.0.19041.0\win-x64\publish"
$AppExe = Join-Path $PublishDir "WinFaceUnlock.exe"

if (-not $NoBuild) {
    & dotnet publish $ProjectPath -c $Configuration -p:Platform=x64 -r win-x64 --self-contained true
    if ($LASTEXITCODE -ne 0) {
        throw "WinFaceUnlock control app publish failed with exit code $LASTEXITCODE"
    }
}

if (-not (Test-Path $AppExe -PathType Leaf)) {
    throw "WinFaceUnlock control app executable is missing: $AppExe"
}

Start-Process -FilePath $AppExe -WorkingDirectory $PublishDir
Write-Host "WinFaceUnlock control app launched:"
Write-Host "  exe: $AppExe"
