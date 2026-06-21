param(
    [string]$PayloadDir = "",
    [string]$ControlAppPublishDir = "",
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..")

if ([string]::IsNullOrWhiteSpace($PayloadDir)) {
    $PayloadDir = Join-Path $RepoRoot "target\setup-payload"
}
$PayloadDir = [System.IO.Path]::GetFullPath($PayloadDir)

if ([string]::IsNullOrWhiteSpace($ControlAppPublishDir)) {
    $winUiConfiguration = if ($Configuration -eq "release") { "Release" } else { "Debug" }
    $ControlAppPublishDir = Join-Path $RepoRoot "apps\setup-winui\WinFaceUnlock.Control.App\bin\x64\$winUiConfiguration\net9.0-windows10.0.19041.0\win-x64\publish"
}
$ControlAppPublishDir = [System.IO.Path]::GetFullPath($ControlAppPublishDir)

if (-not $SkipBuild) {
    $cargoArgs = @("build", "-p", "installer_cli", "-p", "diagnostics_cli", "-p", "win_service", "-p", "windows_provider")
    if ($Configuration -eq "release") {
        $cargoArgs += "--release"
    }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
}

function Resolve-BinaryDir {
    param([string]$Configuration)

    $candidateDirs = @(
        (Join-Path $RepoRoot "target\$Configuration"),
        (Join-Path $RepoRoot "target\x86_64-pc-windows-msvc\$Configuration")
    )
    foreach ($candidateDir in $candidateDirs) {
        if (Test-Path (Join-Path $candidateDir "installer_cli.exe")) {
            return $candidateDir
        }
    }
    throw "Could not find built installer_cli.exe under target directories."
}

function Resolve-OrtRuntimeDllPath {
    param(
        [string]$BinaryDir,
        [string]$ControlAppPublishDir
    )

    $candidatePaths = @(
        (Join-Path $ControlAppPublishDir "onnxruntime.dll"),
        (Join-Path $BinaryDir "onnxruntime.dll"),
        (Join-Path $RepoRoot "target\setup-bundle\app\onnxruntime.dll")
    )
    foreach ($candidatePath in $candidatePaths) {
        if (Test-Path $candidatePath -PathType Leaf) {
            return $candidatePath
        }
    }
    throw "Required ONNX Runtime DLL is missing. Build win_service with the ort dependency or publish the setup bundle first."
}

function Copy-RequiredFile {
    param(
        [string]$SourcePath,
        [string]$TargetRelativePath,
        [System.Collections.ArrayList]$ManifestFiles,
        [string]$FileId = ""
    )

    if (-not (Test-Path $SourcePath -PathType Leaf)) {
        throw "Required payload file is missing: $SourcePath"
    }
    $targetPath = Join-Path $PayloadDir $TargetRelativePath
    $targetParent = Split-Path -Parent $targetPath
    New-Item -ItemType Directory -Path $targetParent -Force | Out-Null
    Copy-Item -LiteralPath $SourcePath -Destination $targetPath -Force
    $manifestFileId = if ([string]::IsNullOrWhiteSpace($FileId)) {
        [System.IO.Path]::GetFileNameWithoutExtension($TargetRelativePath).Replace("-", "_")
    } else {
        $FileId
    }
    [void]$ManifestFiles.Add([ordered]@{
        file_id = $manifestFileId
        source_relative_path = $TargetRelativePath
        target_relative_path = $TargetRelativePath
        required = $true
    })
}

function ConvertTo-ManifestFileId {
    param(
        [string]$Prefix,
        [string]$RelativePath
    )

    $normalized = $RelativePath -replace '[\\/]+', '_'
    $normalized = $normalized -replace '[^A-Za-z0-9_]+', '_'
    $normalized = $normalized.Trim('_')
    if ([string]::IsNullOrWhiteSpace($normalized)) {
        throw "Cannot create a manifest file id from an empty relative path."
    }

    return "$Prefix`_$normalized"
}

function Copy-RequiredDirectoryFiles {
    param(
        [string]$SourceDir,
        [string]$PayloadSourceRootRelativePath,
        [string]$TargetRootRelativePath,
        [string]$FileIdPrefix,
        [System.Collections.ArrayList]$ManifestFiles
    )

    if (-not (Test-Path $SourceDir -PathType Container)) {
        throw "Required payload source directory is missing: $SourceDir"
    }

    $sourceRoot = [System.IO.Path]::GetFullPath($SourceDir).TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
    $files = @(Get-ChildItem -LiteralPath $sourceRoot -File -Recurse | Sort-Object FullName)
    if ($files.Count -eq 0) {
        throw "Required payload source directory contains no files: $SourceDir"
    }

    foreach ($file in $files) {
        $fileFullPath = [System.IO.Path]::GetFullPath($file.FullName)
        $relativePath = $fileFullPath.Substring($sourceRoot.Length).TrimStart([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
        $payloadRelativePath = Join-Path $PayloadSourceRootRelativePath $relativePath
        $targetRelativePath = if ([string]::IsNullOrWhiteSpace($TargetRootRelativePath)) {
            $relativePath
        } else {
            Join-Path $TargetRootRelativePath $relativePath
        }

        $targetPath = Join-Path $PayloadDir $payloadRelativePath
        $targetParent = Split-Path -Parent $targetPath
        New-Item -ItemType Directory -Path $targetParent -Force | Out-Null
        Copy-Item -LiteralPath $file.FullName -Destination $targetPath -Force

        [void]$ManifestFiles.Add([ordered]@{
            file_id = ConvertTo-ManifestFileId -Prefix $FileIdPrefix -RelativePath $targetRelativePath
            source_relative_path = $payloadRelativePath
            target_relative_path = $targetRelativePath
            required = $true
        })
    }
}

function Copy-OptionalFile {
    param(
        [string]$SourcePath,
        [string]$TargetRelativePath,
        [string]$FileId,
        [System.Collections.ArrayList]$ManifestFiles
    )

    if (Test-Path $SourcePath -PathType Leaf) {
        $targetPath = Join-Path $PayloadDir $TargetRelativePath
        $targetParent = Split-Path -Parent $targetPath
        New-Item -ItemType Directory -Path $targetParent -Force | Out-Null
        Copy-Item -LiteralPath $SourcePath -Destination $targetPath -Force
    }
    [void]$ManifestFiles.Add([ordered]@{
        file_id = $FileId
        source_relative_path = $TargetRelativePath
        target_relative_path = $TargetRelativePath
        required = $false
    })
}

function Write-RecoveryScript {
    param(
        [string]$TargetRelativePath,
        [string[]]$Lines,
        [System.Collections.ArrayList]$ManifestFiles
    )

    $targetPath = Join-Path $PayloadDir $TargetRelativePath
    $targetParent = Split-Path -Parent $targetPath
    New-Item -ItemType Directory -Path $targetParent -Force | Out-Null
    Set-Content -LiteralPath $targetPath -Value $Lines -Encoding ASCII
    [void]$ManifestFiles.Add([ordered]@{
        file_id = [System.IO.Path]::GetFileNameWithoutExtension($TargetRelativePath).Replace("-", "_")
        source_relative_path = $TargetRelativePath
        target_relative_path = $TargetRelativePath
        required = $true
    })
}

Remove-Item -LiteralPath $PayloadDir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $PayloadDir -Force | Out-Null
$BinaryDir = Resolve-BinaryDir -Configuration $Configuration
$ManifestFiles = [System.Collections.ArrayList]::new()

Copy-RequiredFile -SourcePath (Join-Path $BinaryDir "installer_cli.exe") -TargetRelativePath "installer_cli.exe" -ManifestFiles $ManifestFiles
Copy-RequiredFile -SourcePath (Join-Path $BinaryDir "diagnostics_cli.exe") -TargetRelativePath "diagnostics_cli.exe" -ManifestFiles $ManifestFiles
Copy-RequiredFile -SourcePath (Join-Path $BinaryDir "win_service.exe") -TargetRelativePath "win_service.exe" -ManifestFiles $ManifestFiles
$ProviderDllSourcePath = Join-Path $BinaryDir "windows_provider.dll"
$ProviderDllHash = (Get-FileHash -LiteralPath $ProviderDllSourcePath -Algorithm SHA256).Hash.Substring(0, 12).ToLowerInvariant()
$ProviderDllTargetRelativePath = "provider\windows_provider-$ProviderDllHash.dll"
Copy-RequiredFile -SourcePath $ProviderDllSourcePath -TargetRelativePath $ProviderDllTargetRelativePath -ManifestFiles $ManifestFiles -FileId "windows_provider"
Copy-RequiredDirectoryFiles `
    -SourceDir $ControlAppPublishDir `
    -PayloadSourceRootRelativePath "control-app" `
    -TargetRootRelativePath "" `
    -FileIdPrefix "control_app" `
    -ManifestFiles $ManifestFiles

$ModelsDir = Join-Path $RepoRoot "models"
Copy-RequiredFile -SourcePath (Join-Path $ModelsDir "face_detection_yunet_2023mar.onnx") -TargetRelativePath "models\face_detection_yunet_2023mar.onnx" -ManifestFiles $ManifestFiles
Copy-RequiredFile -SourcePath (Join-Path $ModelsDir "face_recognition_sface_2021dec.onnx") -TargetRelativePath "models\face_recognition_sface_2021dec.onnx" -ManifestFiles $ManifestFiles
Copy-RequiredFile -SourcePath (Join-Path $ModelsDir "minifasnet_v2.onnx") -TargetRelativePath "models\minifasnet_v2.onnx" -ManifestFiles $ManifestFiles
Copy-RequiredFile -SourcePath (Join-Path $ModelsDir "yolov8n.onnx") -TargetRelativePath "models\yolov8n.onnx" -FileId "yolov8_person_model" -ManifestFiles $ManifestFiles
Copy-OptionalFile -SourcePath (Join-Path $ModelsDir "MobileNetSSD_deploy.caffemodel") -TargetRelativePath "models\MobileNetSSD_deploy.caffemodel" -FileId "mobilenet_ssd_person_model" -ManifestFiles $ManifestFiles
Copy-OptionalFile -SourcePath (Join-Path $ModelsDir "MobileNetSSD_deploy.prototxt") -TargetRelativePath "models\MobileNetSSD_deploy.prototxt" -FileId "mobilenet_ssd_person_config" -ManifestFiles $ManifestFiles
Copy-OptionalFile -SourcePath (Join-Path $ModelsDir "pose_landmarker_lite.task") -TargetRelativePath "models\pose_landmarker_lite.task" -FileId "mediapipe_pose_lite_model" -ManifestFiles $ManifestFiles
Copy-OptionalFile -SourcePath (Join-Path $RepoRoot "native\winfaceunlock_mediapipe_bridge.dll") -TargetRelativePath "native\winfaceunlock_mediapipe_bridge.dll" -FileId "mediapipe_bridge" -ManifestFiles $ManifestFiles

$RuntimeDllDir = Join-Path $RepoRoot "vcpkg_installed\x64-windows\bin"
if (-not (Test-Path $RuntimeDllDir -PathType Container)) {
    throw "Required runtime DLL directory is missing: $RuntimeDllDir"
}
$OrtRuntimeDllPath = Resolve-OrtRuntimeDllPath -BinaryDir $BinaryDir -ControlAppPublishDir $ControlAppPublishDir
Copy-RequiredFile -SourcePath $OrtRuntimeDllPath -TargetRelativePath "onnxruntime.dll" -FileId "onnxruntime" -ManifestFiles $ManifestFiles
Get-ChildItem -LiteralPath $RuntimeDllDir -Filter "*.dll" | Sort-Object Name | ForEach-Object {
    $runtimeFileName = $_.Name
    $runtimeFileId = "runtime_" + [System.IO.Path]::GetFileNameWithoutExtension($runtimeFileName).Replace("-", "_").Replace(".", "_")
    Copy-RequiredFile -SourcePath $_.FullName -TargetRelativePath $runtimeFileName -ManifestFiles $ManifestFiles
}

Write-RecoveryScript -TargetRelativePath "recovery\emergency-disable.cmd" -ManifestFiles $ManifestFiles -Lines @(
    "@echo off",
    "set ROOT=%~dp0..",
    'call "%ROOT%\installer_cli.exe" emergency-disable'
)
Write-RecoveryScript -TargetRelativePath "recovery\uninstall.cmd" -ManifestFiles $ManifestFiles -Lines @(
    "@echo off",
    "set ROOT=%~dp0..",
    'call "%ROOT%\installer_cli.exe" uninstall'
)
Write-RecoveryScript -TargetRelativePath "recovery\repair.cmd" -ManifestFiles $ManifestFiles -Lines @(
    "@echo off",
    "set ROOT=%~dp0..",
    "call ""%ROOT%\installer_cli.exe"" repair --service-binary ""%ROOT%\win_service.exe"" --provider-binary ""%ROOT%\$ProviderDllTargetRelativePath"" --start-service"
)

[void]$ManifestFiles.Add([ordered]@{
    file_id = "payload_manifest"
    source_relative_path = "winfaceunlock-payload.json"
    target_relative_path = "winfaceunlock-payload.json"
    required = $true
})

$Manifest = [ordered]@{
    manifest_version = 1
    payload_files = $ManifestFiles
}
$ManifestPath = Join-Path $PayloadDir "winfaceunlock-payload.json"
$Manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $ManifestPath -Encoding ASCII

Write-Host "WinFaceUnlock setup payload built:"
Write-Host "  payload_dir: $PayloadDir"
Write-Host "  manifest: $ManifestPath"
