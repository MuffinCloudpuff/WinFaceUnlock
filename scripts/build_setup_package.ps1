param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [switch]$SkipBundleBuild,
    [switch]$SkipWinUiBuild,
    [switch]$SkipTauriBuild
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..")
$BundleScript = Join-Path $ScriptDir "build_setup_bundle.ps1"
$BundleRoot = Join-Path $RepoRoot "target\setup-bundle"
$PackageRoot = Join-Path $RepoRoot "target\setup-package"
$BundleZip = Join-Path $PackageRoot "setup-bundle.zip"
$BootstrapperOutput = Join-Path $PackageRoot "WinFaceUnlockSetup.exe"
$PackageManifest = Join-Path $PackageRoot "WinFaceUnlockSetup.package.json"

$env:Path = "C:\Users\Liu\.cargo\bin;" + $env:Path

if (-not $SkipBundleBuild) {
    $bundleArgs = @("-ExecutionPolicy", "Bypass", "-File", $BundleScript, "-Configuration", $Configuration)
    if ($SkipWinUiBuild) {
        $bundleArgs += "-SkipWinUiBuild"
    }
    if ($SkipTauriBuild) {
        $bundleArgs += "-SkipTauriBuild"
    }

    & powershell @bundleArgs
    if ($LASTEXITCODE -ne 0) {
        throw "Setup bundle build failed with exit code $LASTEXITCODE"
    }
}

if (-not (Test-Path $BundleRoot -PathType Container)) {
    throw "Setup bundle directory is missing: $BundleRoot"
}

Remove-Item -LiteralPath $PackageRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $PackageRoot -Force | Out-Null

Compress-Archive -Path (Join-Path $BundleRoot "*") -DestinationPath $BundleZip -CompressionLevel Optimal -Force

$previousBundleZip = $env:WINFACEUNLOCK_SETUP_BUNDLE_ZIP
$env:WINFACEUNLOCK_SETUP_BUNDLE_ZIP = $BundleZip
try {
    & cargo build -p setup_bootstrapper --release
    if ($LASTEXITCODE -ne 0) {
        throw "Setup bootstrapper build failed with exit code $LASTEXITCODE"
    }
}
finally {
    if ($null -eq $previousBundleZip) {
        Remove-Item Env:\WINFACEUNLOCK_SETUP_BUNDLE_ZIP -ErrorAction SilentlyContinue
    }
    else {
        $env:WINFACEUNLOCK_SETUP_BUNDLE_ZIP = $previousBundleZip
    }
}

$BootstrapperCandidates = @(
    (Join-Path $RepoRoot "target\release\setup_bootstrapper.exe"),
    (Join-Path $RepoRoot "target\x86_64-pc-windows-msvc\release\setup_bootstrapper.exe")
)
$BootstrapperSource = $BootstrapperCandidates | Where-Object {
    Test-Path $_ -PathType Leaf
} | Select-Object -First 1

if ($null -eq $BootstrapperSource) {
    throw "Setup bootstrapper output is missing. Checked: $($BootstrapperCandidates -join '; ')"
}

Copy-Item -LiteralPath $BootstrapperSource -Destination $BootstrapperOutput -Force

$bundleHash = Get-FileHash -Path $BundleZip -Algorithm SHA256
$setupHash = Get-FileHash -Path $BootstrapperOutput -Algorithm SHA256
$packageInfo = [ordered]@{
    package_name = "WinFaceUnlockSetup.exe"
    configuration = $Configuration
    bundle_zip_sha256 = $bundleHash.Hash.ToLowerInvariant()
    setup_exe_sha256 = $setupHash.Hash.ToLowerInvariant()
    setup_exe_size_bytes = (Get-Item $BootstrapperOutput).Length
    bundle_zip_size_bytes = (Get-Item $BundleZip).Length
    app_entrypoint = "app\WinFaceUnlock.Setup.App.exe"
    payload_manifest = "payload\winfaceunlock-payload.json"
}

$packageInfo | ConvertTo-Json -Depth 4 | Set-Content -Path $PackageManifest -Encoding UTF8

Write-Host "WinFaceUnlock setup package built:"
Write-Host "  package_root: $PackageRoot"
Write-Host "  setup_exe: $BootstrapperOutput"
Write-Host "  manifest: $PackageManifest"
