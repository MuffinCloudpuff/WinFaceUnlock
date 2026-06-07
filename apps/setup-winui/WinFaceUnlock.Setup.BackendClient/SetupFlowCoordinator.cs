using System;
using System.Text.Json;

namespace WinFaceUnlock.Setup.BackendClient;

public sealed class SetupFlowCoordinator
{
    private readonly SetupBackendClient _client;

    public SetupFlowCoordinator(SetupBackendClient client)
    {
        _client = client;
    }

    public IReadOnlyList<StagePayloadFile> LastInspectedStageFiles { get; private set; } = Array.Empty<StagePayloadFile>();
    public string LastInspectedPayloadRootDir { get; private set; } = "";

    public IReadOnlyList<SetupFlowPlanStep> CreateReadinessCheckPlan(SetupReadinessCheckOptions options)
    {
        return
        [
            new("inspect", "inspect_package", token => InspectPayloadAsync(options.PayloadRootDir, token)),
            new(
                "preflight",
                "source_preflight",
                token => RunSourcePayloadPreflightAsync(options.InstallDir, requireElevation: true, token)),
        ];
    }

    public IReadOnlyList<SetupFlowPlanStep> CreateInstallPlan(SetupInstallPlanOptions options)
    {
        return
        [
            new("inspect", "inspect_package", token => InspectPayloadAsync(options.PayloadRootDir, token)),
            new(
                "preflight",
                "source_preflight",
                token => RunSourcePayloadPreflightAsync(options.InstallDir, requireElevation: true, token)),
            new(
                "stage",
                "stage_payload",
                token => StagePayloadAsync(
                    options.InstallDir,
                    options.PayloadRootDir,
                    overwriteExisting: true,
                    token)),
            new(
                "preflight",
                "staged_preflight",
                token => RunStagedPayloadPreflightAsync(options.InstallDir, requireElevation: true, token)),
            new(
                "install",
                "install_components",
                token => InstallSystemComponentsAsync(
                    options.InstallDir,
                    cameraId: "opencv-index:0",
                    configureLocalCameraAuth: false,
                    matchThreshold: null,
                    requiredConsecutiveMatchCount: null,
                    token)),
        ];
    }

    public IReadOnlyList<SetupFlowPlanStep> CreateRepairPlan(SetupRepairPlanOptions options)
    {
        return
        [
            new("inspect", "inspect_package", token => InspectPayloadAsync(options.PayloadRootDir, token)),
            new(
                "preflight",
                "source_preflight",
                token => RunSourcePayloadPreflightAsync(options.InstallDir, requireElevation: true, token)),
            new(
                "stage",
                "stage_payload",
                token => StagePayloadAsync(
                    options.InstallDir,
                    options.PayloadRootDir,
                    overwriteExisting: true,
                    token)),
            new(
                "preflight",
                "staged_preflight",
                token => RunStagedPayloadPreflightAsync(options.InstallDir, requireElevation: true, token)),
            new(
                "recovery",
                "repair_components",
                token => RepairSystemComponentsAsync(
                    options.InstallDir,
                    cameraId: "opencv-index:0",
                    matchThreshold: null,
                    requiredConsecutiveMatchCount: null,
                    token)),
        ];
    }

    public Task<SetupResponseEnvelope> GetStatusAsync(CancellationToken cancellationToken)
    {
        return _client.SendAsync(SetupOperations.GetStatus, new { }, cancellationToken);
    }

    public async Task<SetupResponseEnvelope> InspectPayloadAsync(
        string payloadRootDir,
        CancellationToken cancellationToken)
    {
        var response = await _client.SendAsync(
            SetupOperations.InspectPayload,
            new InspectPayloadPayload(payloadRootDir),
            cancellationToken);

        LastInspectedStageFiles = response.Succeeded
            ? ReadStagePayloadFiles(response.SafeDetails)
            : Array.Empty<StagePayloadFile>();
        LastInspectedPayloadRootDir = response.Succeeded ? payloadRootDir : "";
        return response;
    }

    public Task<SetupResponseEnvelope> RunSourcePayloadPreflightAsync(
        string installDir,
        bool requireElevation,
        CancellationToken cancellationToken)
    {
        if (LastInspectedStageFiles.Count == 0)
        {
            throw new InvalidOperationException("Inspect the payload before running source payload preflight.");
        }

        return _client.SendAsync(
            SetupOperations.RunPreflight,
            new PreflightPayload(
                installDir,
                requireElevation,
                LastInspectedStageFiles
                    .Select(file => new RequiredPayloadFile(
                        file.FileId,
                        ResolvePayloadSourcePath(LastInspectedPayloadRootDir, file.SourcePath)))
                    .ToArray()),
            cancellationToken);
    }

    public Task<SetupResponseEnvelope> RunStagedPayloadPreflightAsync(
        string installDir,
        bool requireElevation,
        CancellationToken cancellationToken)
    {
        if (LastInspectedStageFiles.Count == 0)
        {
            throw new InvalidOperationException("Inspect the payload before running staged payload preflight.");
        }

        return _client.SendAsync(
            SetupOperations.RunPreflight,
            new PreflightPayload(
                installDir,
                requireElevation,
                LastInspectedStageFiles
                    .Select(file => new RequiredPayloadFile(
                        file.FileId,
                        Path.Combine(installDir, file.TargetRelativePath)))
                    .ToArray()),
            cancellationToken);
    }

    public Task<SetupResponseEnvelope> StagePayloadAsync(
        string installDir,
        string payloadRootDir,
        bool overwriteExisting,
        CancellationToken cancellationToken)
    {
        if (LastInspectedStageFiles.Count == 0)
        {
            throw new InvalidOperationException("Inspect the payload before staging it.");
        }

        return _client.SendAsync(
            SetupOperations.StagePayload,
            new StagePayloadPayload(installDir, payloadRootDir, overwriteExisting, LastInspectedStageFiles),
            cancellationToken);
    }

    public Task<SetupResponseEnvelope> InstallSystemComponentsAsync(
        string installDir,
        string cameraId,
        bool configureLocalCameraAuth,
        float? matchThreshold,
        uint? requiredConsecutiveMatchCount,
        CancellationToken cancellationToken)
    {
        return _client.SendAsync(
            SetupOperations.InstallSystemComponents,
            new InstallSystemComponentsPayload
            {
                InstallDir = installDir,
                CameraId = cameraId,
                ConfigureLocalCameraAuth = configureLocalCameraAuth,
                MatchThreshold = matchThreshold,
                RequiredConsecutiveMatchCount = requiredConsecutiveMatchCount
            },
            cancellationToken);
    }

    public Task<SetupResponseEnvelope> RepairSystemComponentsAsync(
        string installDir,
        string cameraId,
        float? matchThreshold,
        uint? requiredConsecutiveMatchCount,
        CancellationToken cancellationToken)
    {
        return _client.SendAsync(
            SetupOperations.Repair,
            new InstallSystemComponentsPayload
            {
                InstallDir = installDir,
                CameraId = cameraId,
                MatchThreshold = matchThreshold,
                RequiredConsecutiveMatchCount = requiredConsecutiveMatchCount
            },
            cancellationToken);
    }

    public async Task<SetupResponseEnvelope> EnrollCredentialAsync(
        string username,
        string userId,
        string userSid,
        string accountType,
        string password,
        CancellationToken cancellationToken)
    {
        using var secretPipe = CredentialSecretPipe.Create();
        using var linkedCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        linkedCts.CancelAfter(TimeSpan.FromMilliseconds((double)secretPipe.TimeoutMs + 5_000));

        var responseTask = _client.SendAsync(
            SetupOperations.EnrollCredential,
            new EnrollCredentialPayload
            {
                Username = username,
                UserId = userId,
                UserSid = userSid,
                AccountType = accountType,
                PasswordSecretTransport = secretPipe.TransportPayload
            },
            linkedCts.Token);
        var secretTask = secretPipe.WriteCredentialSecretAsync(password, linkedCts.Token);

        try
        {
            var firstCompleted = await Task.WhenAny(responseTask, secretTask);
            if (firstCompleted == secretTask && secretTask.IsFaulted)
            {
                linkedCts.Cancel();
                await responseTask;
            }

            var response = await responseTask;
            if (!response.Succeeded)
            {
                linkedCts.Cancel();
                await ObservePipeWriterAsync(secretTask);
                return response;
            }

            await secretTask;
            return response;
        }
        catch
        {
            linkedCts.Cancel();
            await ObservePipeWriterAsync(secretTask);
            throw;
        }
    }

    public Task<SetupResponseEnvelope> EnrollFaceTemplateAsync(
        string installDir,
        string cameraId,
        bool allowPartialEnrollment,
        CancellationToken cancellationToken)
    {
        return _client.SendAsync(
            SetupOperations.EnrollFaceTemplate,
            new EnrollFaceTemplatePayload
            {
                InstallDir = installDir,
                CameraId = cameraId,
                AllowPartialEnrollment = allowPartialEnrollment
            },
            cancellationToken);
    }

    public Task<SetupResponseEnvelope> RunAuthSelfTestAsync(
        string installDir,
        CancellationToken cancellationToken)
    {
        return _client.SendAsync(
            SetupOperations.RunAuthSelfTest,
            new RunAuthSelfTestPayload
            {
                InstallDir = installDir
            },
            cancellationToken);
    }

    public Task<SetupResponseEnvelope> EmergencyDisableAsync(CancellationToken cancellationToken)
    {
        return _client.SendAsync(SetupOperations.EmergencyDisable, new { }, cancellationToken);
    }

    public Task<SetupResponseEnvelope> UninstallAsync(
        bool preserveData,
        CancellationToken cancellationToken)
    {
        return _client.SendAsync(
            SetupOperations.Uninstall,
            new UninstallPayload(preserveData),
            cancellationToken);
    }

    private static IReadOnlyList<StagePayloadFile> ReadStagePayloadFiles(JsonElement safeDetails)
    {
        if (!safeDetails.TryGetProperty("stage_payload_files", out var filesElement))
        {
            return Array.Empty<StagePayloadFile>();
        }

        var files = filesElement.Deserialize<IReadOnlyList<StagePayloadFile>>();
        return files ?? Array.Empty<StagePayloadFile>();
    }

    private static async Task ObservePipeWriterAsync(Task pipeWriter)
    {
        try
        {
            await pipeWriter;
        }
        catch (OperationCanceledException)
        {
        }
    }

    internal static string ResolvePayloadSourcePath(string payloadRootDir, string sourcePath)
    {
        if (Path.IsPathRooted(sourcePath))
        {
            return sourcePath;
        }

        if (string.IsNullOrWhiteSpace(payloadRootDir))
        {
            throw new InvalidOperationException("Payload root directory is required for relative payload source paths.");
        }

        return Path.Combine(payloadRootDir, sourcePath);
    }
}

public sealed record SetupFlowPlanStep(
    string StepKey,
    string RunningMessageKey,
    Func<CancellationToken, Task<SetupResponseEnvelope>> ExecuteAsync);

public sealed record SetupReadinessCheckOptions(
    string InstallDir,
    string PayloadRootDir);

public sealed record SetupInstallPlanOptions
{
    public string InstallDir { get; init; } = "";
    public string PayloadRootDir { get; init; } = "";
}

public sealed record SetupRepairPlanOptions
{
    public string InstallDir { get; init; } = "";
    public string PayloadRootDir { get; init; } = "";
}
