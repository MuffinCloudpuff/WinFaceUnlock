param(
    [switch]$SkipBuild,
    [string]$SetupExe = "",
    [string]$StageDir = ""
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..")
$PackageScript = Join-Path $ScriptDir "build_setup_package.ps1"

if ([string]::IsNullOrWhiteSpace($SetupExe)) {
    $SetupExe = Join-Path $RepoRoot "target\setup-package\WinFaceUnlockSetup.exe"
}
$SetupExe = [System.IO.Path]::GetFullPath($SetupExe)

if (-not $SkipBuild) {
    & powershell -ExecutionPolicy Bypass -File $PackageScript
    if ($LASTEXITCODE -ne 0) {
        throw "Setup package build failed with exit code $LASTEXITCODE"
    }
}

if (-not (Test-Path $SetupExe -PathType Leaf)) {
    throw "Setup package executable is missing: $SetupExe"
}

$bootstrapperReport = Join-Path $env:TEMP ("WinFaceUnlockBootstrapperValidate-" + [System.Guid]::NewGuid().ToString("N") + ".txt")
$previousBootstrapperReport = $env:WINFACEUNLOCK_BOOTSTRAPPER_VALIDATE_OUTPUT
$env:WINFACEUNLOCK_BOOTSTRAPPER_VALIDATE_OUTPUT = $bootstrapperReport
try {
    $bootstrapperProcess = Start-Process `
        -FilePath $SetupExe `
        -ArgumentList "--winfaceunlock-bootstrapper-validate" `
        -Wait `
        -PassThru
    if ($bootstrapperProcess.ExitCode -ne 0) {
        throw "Bootstrapper validation failed with exit code $($bootstrapperProcess.ExitCode)"
    }

    if (Test-Path $bootstrapperReport -PathType Leaf) {
        $bootstrapperOutput = Get-Content -LiteralPath $bootstrapperReport
    } else {
        $bootstrapperOutput = @()
    }
}
finally {
    if ($null -eq $previousBootstrapperReport) {
        Remove-Item Env:\WINFACEUNLOCK_BOOTSTRAPPER_VALIDATE_OUTPUT -ErrorAction SilentlyContinue
    } else {
        $env:WINFACEUNLOCK_BOOTSTRAPPER_VALIDATE_OUTPUT = $previousBootstrapperReport
    }
    Remove-Item -LiteralPath $bootstrapperReport -Force -ErrorAction SilentlyContinue
}

$bootstrapperFields = @{}
$bootstrapperOutput | ForEach-Object {
    $line = [string]$_
    $separator = $line.IndexOf("=")
    if ($separator -gt 0) {
        $bootstrapperFields[$line.Substring(0, $separator)] = $line.Substring($separator + 1)
    }
}

if ($bootstrapperFields["winfaceunlock_bootstrapper_validation"] -ne "succeeded") {
    throw "Bootstrapper did not report a successful validation marker."
}

$payloadRootDir = $bootstrapperFields["payload_root_dir"]
$backendExe = $bootstrapperFields["backend_exe"]
$appEntrypoint = $bootstrapperFields["app_entrypoint"]

foreach ($requiredPath in @($payloadRootDir, $backendExe, $appEntrypoint)) {
    if ([string]::IsNullOrWhiteSpace($requiredPath)) {
        throw "Bootstrapper validation output is missing a required path."
    }
}

if (-not (Test-Path $payloadRootDir -PathType Container)) {
    throw "Extracted payload root is missing: $payloadRootDir"
}
if (-not (Test-Path $backendExe -PathType Leaf)) {
    throw "Extracted setup backend is missing: $backendExe"
}
if (-not (Test-Path $appEntrypoint -PathType Leaf)) {
    throw "Extracted WinUI entrypoint is missing: $appEntrypoint"
}

function Invoke-SetupBackend {
    param([hashtable]$Request)

    $requestJson = $Request | ConvertTo-Json -Depth 20 -Compress
    $requestFile = Join-Path $env:TEMP ("WinFaceUnlockSetupBackendRequest-" + [System.Guid]::NewGuid().ToString("N") + ".json")
    try {
        [System.IO.File]::WriteAllText($requestFile, $requestJson, [System.Text.UTF8Encoding]::new($false))
        $commandLine = "`"$backendExe`" setup-backend < `"$requestFile`""
        $output = & cmd.exe /D /C $commandLine 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        Remove-Item -LiteralPath $requestFile -Force -ErrorAction SilentlyContinue
    }

    if ($exitCode -ne 0) {
        $output | ForEach-Object { Write-Host $_ }
        throw "setup-backend failed with exit code $exitCode for operation $($Request.operation)"
    }

    $jsonLine = $output |
        ForEach-Object { [string]$_ } |
        Where-Object { $_.Trim().StartsWith("{") -and $_.Trim().EndsWith("}") } |
        Select-Object -Last 1
    if ([string]::IsNullOrWhiteSpace($jsonLine)) {
        $output | ForEach-Object { Write-Host $_ }
        throw "setup-backend did not return a JSON response for operation $($Request.operation)"
    }

    return $jsonLine | ConvertFrom-Json
}

function Assert-SetupSucceeded {
    param(
        [object]$Response,
        [string]$Operation
    )

    if ($Response.operation_status -ne "succeeded") {
        $Response | ConvertTo-Json -Depth 20 | Write-Host
        throw "Setup backend operation $Operation did not succeed: $($Response.operation_status)"
    }
}

function Resolve-PayloadSourcePath {
    param(
        [string]$PayloadRootDir,
        [string]$SourcePath
    )

    if ([System.IO.Path]::IsPathRooted($SourcePath)) {
        return $SourcePath
    }

    return Join-Path $PayloadRootDir $SourcePath
}

$inspectRequest = @{
    protocol_version = 1
    correlation_id = "validate-package-inspect"
    operation = "inspect_payload"
    payload = @{
        payload_root_dir = $payloadRootDir
    }
}
$inspectResponse = Invoke-SetupBackend -Request $inspectRequest
Assert-SetupSucceeded -Response $inspectResponse -Operation "inspect_payload"

$stagePayloadFiles = @($inspectResponse.safe_details.stage_payload_files)
if ($stagePayloadFiles.Count -eq 0) {
    throw "inspect_payload returned no stage payload files."
}
if (-not ($stagePayloadFiles | Where-Object { $_.target_relative_path -eq "WinFaceUnlock.exe" })) {
    throw "inspect_payload did not include the installed control panel entrypoint WinFaceUnlock.exe."
}
if (-not ($stagePayloadFiles | Where-Object { $_.target_relative_path -eq "desktop_input_agent.exe" })) {
    throw "inspect_payload did not include DesktopInputPresenceAgent desktop_input_agent.exe."
}

$sourceRequiredFiles = @(
    $stagePayloadFiles | ForEach-Object {
        @{
            file_id = $_.file_id
            path = Resolve-PayloadSourcePath -PayloadRootDir $payloadRootDir -SourcePath $_.source_path
        }
    }
)
$sourcePreflightInstallDir = Join-Path $env:TEMP "WinFaceUnlock"
$sourcePreflightResponse = Invoke-SetupBackend -Request @{
    protocol_version = 1
    correlation_id = "validate-package-source-preflight"
    operation = "run_preflight"
    payload = @{
        install_dir = $sourcePreflightInstallDir
        require_elevation = $false
        required_payload_files = $sourceRequiredFiles
    }
}
Assert-SetupSucceeded -Response $sourcePreflightResponse -Operation "run_preflight(source)"

$stageDirWasGenerated = [string]::IsNullOrWhiteSpace($StageDir)
if ($stageDirWasGenerated) {
    $StageRootDir = Join-Path $env:TEMP ("WinFaceUnlockSetupValidate-" + [System.Guid]::NewGuid().ToString("N"))
    $StageDir = Join-Path $StageRootDir "WinFaceUnlock"
    New-Item -ItemType Directory -Path $StageRootDir -Force | Out-Null
}
$StageDir = [System.IO.Path]::GetFullPath($StageDir)
if (Test-Path $StageDir) {
    if (-not $stageDirWasGenerated) {
        throw "Explicit StageDir already exists; choose an empty/nonexistent validation directory: $StageDir"
    }
    if ((Split-Path -Leaf $StageDir) -ne "WinFaceUnlock") {
        throw "Refusing to remove unexpected generated StageDir: $StageDir"
    }
    $stageRoot = Split-Path -Parent $StageDir
    if ((Split-Path -Leaf $stageRoot) -notlike "WinFaceUnlockSetupValidate-*") {
        throw "Refusing to remove unexpected generated StageRootDir: $stageRoot"
    }
    Remove-Item -LiteralPath $stageRoot -Recurse -Force
}

$stageResponse = Invoke-SetupBackend -Request @{
    protocol_version = 1
    correlation_id = "validate-package-stage"
    operation = "stage_payload"
    payload = @{
        install_dir = $StageDir
        payload_root_dir = $payloadRootDir
        overwrite_existing = $false
        payload_files = $stagePayloadFiles
    }
}
Assert-SetupSucceeded -Response $stageResponse -Operation "stage_payload"

$stagedRequiredFiles = @(
    $stagePayloadFiles | ForEach-Object {
        @{
            file_id = $_.file_id
            path = Join-Path $StageDir $_.target_relative_path
        }
    }
)
$stagedPreflightResponse = Invoke-SetupBackend -Request @{
    protocol_version = 1
    correlation_id = "validate-package-staged-preflight"
    operation = "run_preflight"
    payload = @{
        install_dir = $StageDir
        require_elevation = $false
        required_payload_files = $stagedRequiredFiles
    }
}
Assert-SetupSucceeded -Response $stagedPreflightResponse -Operation "run_preflight(staged)"

$installedControlApp = Join-Path $StageDir "WinFaceUnlock.exe"
if (-not (Test-Path $installedControlApp -PathType Leaf)) {
    throw "Staged install is missing the control panel entrypoint: $installedControlApp"
}
$installedTauriLibrary = Join-Path $StageDir "winfaceunlock_control_tauri_lib.dll"
if (-not (Test-Path $installedTauriLibrary -PathType Leaf)) {
    throw "Staged install is missing the Tauri control library: $installedTauriLibrary"
}
$installedDesktopInputAgent = Join-Path $StageDir "desktop_input_agent.exe"
if (-not (Test-Path $installedDesktopInputAgent -PathType Leaf)) {
    throw "Staged install is missing DesktopInputPresenceAgent: $installedDesktopInputAgent"
}

Write-Host "WinFaceUnlock setup package validation passed:"
Write-Host "  setup_exe: $SetupExe"
Write-Host "  app_entrypoint: $appEntrypoint"
Write-Host "  backend_exe: $backendExe"
Write-Host "  payload_root_dir: $payloadRootDir"
Write-Host "  staged_validation_dir: $StageDir"
Write-Host "  staged_control_app: $installedControlApp"
