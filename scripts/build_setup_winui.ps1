param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..")
$SolutionPath = Join-Path $RepoRoot "apps\setup-winui\WinFaceUnlock.Setup.sln"
$SetupAppProjectPath = Join-Path $RepoRoot "apps\setup-winui\WinFaceUnlock.Setup.App\WinFaceUnlock.Setup.App.csproj"
$ControlAppProjectPath = Join-Path $RepoRoot "apps\setup-winui\WinFaceUnlock.Control.App\WinFaceUnlock.Control.App.csproj"

if (-not (Test-Path $SolutionPath -PathType Leaf)) {
    throw "WinUI setup solution is missing: $SolutionPath"
}

$dotnetInfo = & dotnet --info 2>&1
if ($LASTEXITCODE -ne 0 -or ($dotnetInfo -join "`n") -match "No SDKs were found") {
    throw "A .NET SDK with WinUI build support is required before building the setup app."
}

& dotnet build $SolutionPath -c $Configuration -p:Platform=x64
if ($LASTEXITCODE -ne 0) {
    throw "WinUI setup build failed with exit code $LASTEXITCODE"
}

& dotnet publish $SetupAppProjectPath -c $Configuration -p:Platform=x64 -r win-x64 --self-contained true
if ($LASTEXITCODE -ne 0) {
    throw "WinUI setup publish failed with exit code $LASTEXITCODE"
}

& dotnet publish $ControlAppProjectPath -c $Configuration -p:Platform=x64 -r win-x64 --self-contained true
if ($LASTEXITCODE -ne 0) {
    throw "WinUI control publish failed with exit code $LASTEXITCODE"
}
