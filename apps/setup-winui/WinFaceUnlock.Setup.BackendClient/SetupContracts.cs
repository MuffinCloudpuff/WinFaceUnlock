using System.Text.Json;
using System.Text.Json.Serialization;

namespace WinFaceUnlock.Setup.BackendClient;

public static class SetupOperations
{
    public const string GetStatus = "get_status";
    public const string RunPreflight = "run_preflight";
    public const string InspectPayload = "inspect_payload";
    public const string StagePayload = "stage_payload";
    public const string EnrollCredential = "enroll_credential";
    public const string EnrollFaceTemplate = "enroll_face_template";
    public const string RunAuthSelfTest = "run_auth_self_test";
    public const string InstallSystemComponents = "install_system_components";
    public const string ConfigurePresenceLock = "configure_presence_lock";
    public const string Repair = "repair";
    public const string EmergencyDisable = "emergency_disable";
    public const string Uninstall = "uninstall";
}

public static class SetupOperationStatus
{
    public const string Succeeded = "succeeded";
    public const string Failed = "failed";
    public const string RequiresElevation = "requires_elevation";
    public const string RequiresUserInput = "requires_user_input";
    public const string BlockedByRunningProcess = "blocked_by_running_process";
    public const string Cancelled = "cancelled";
    public const string UnsupportedProtocol = "unsupported_protocol";
    public const string InvalidRequest = "invalid_request";
}

public sealed record SetupRequestEnvelope(
    [property: JsonPropertyName("protocol_version")] int ProtocolVersion,
    [property: JsonPropertyName("correlation_id")] string CorrelationId,
    [property: JsonPropertyName("operation")] string Operation,
    [property: JsonPropertyName("payload")] object Payload);

public sealed record SetupResponseEnvelope
{
    [JsonPropertyName("protocol_version")]
    public int ProtocolVersion { get; init; }

    [JsonPropertyName("correlation_id")]
    public string CorrelationId { get; init; } = "";

    [JsonPropertyName("operation")]
    public string Operation { get; init; } = "";

    [JsonPropertyName("operation_status")]
    public string OperationStatus { get; init; } = "";

    [JsonPropertyName("message")]
    public string Message { get; init; } = "";

    [JsonPropertyName("safe_details")]
    public JsonElement SafeDetails { get; init; }

    [JsonPropertyName("next_recommended_action")]
    public string? NextRecommendedAction { get; init; }

    public bool Succeeded => OperationStatus == SetupOperationStatus.Succeeded;
}

public sealed record InspectPayloadPayload(
    [property: JsonPropertyName("payload_root_dir")] string PayloadRootDir,
    [property: JsonPropertyName("manifest_relative_path")] string ManifestRelativePath = "winfaceunlock-payload.json");

public sealed record PreflightPayload(
    [property: JsonPropertyName("install_dir")] string InstallDir,
    [property: JsonPropertyName("require_elevation")] bool RequireElevation,
    [property: JsonPropertyName("required_payload_files")] IReadOnlyList<RequiredPayloadFile> RequiredPayloadFiles);

public sealed record RequiredPayloadFile(
    [property: JsonPropertyName("file_id")] string FileId,
    [property: JsonPropertyName("path")] string Path);

public sealed record StagePayloadPayload(
    [property: JsonPropertyName("install_dir")] string InstallDir,
    [property: JsonPropertyName("payload_root_dir")] string PayloadRootDir,
    [property: JsonPropertyName("overwrite_existing")] bool OverwriteExisting,
    [property: JsonPropertyName("payload_files")] IReadOnlyList<StagePayloadFile> PayloadFiles);

public sealed record StagePayloadFile
{
    [JsonPropertyName("file_id")]
    public string FileId { get; init; } = "";

    [JsonPropertyName("source_path")]
    public string SourcePath { get; init; } = "";

    [JsonPropertyName("target_relative_path")]
    public string TargetRelativePath { get; init; } = "";
}

public sealed record EnrollCredentialPayload
{
    [JsonPropertyName("install_dir")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? InstallDir { get; init; }

    [JsonPropertyName("username")]
    public string Username { get; init; } = "";

    [JsonPropertyName("user_id")]
    public string UserId { get; init; } = "dev-user";

    [JsonPropertyName("user_sid")]
    public string UserSid { get; init; } = "S-1-5-21-winfaceunlock-pending";

    [JsonPropertyName("account_type")]
    public string AccountType { get; init; } = "local";

    [JsonPropertyName("credential_ref")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? CredentialRef { get; init; }

    [JsonPropertyName("store_dir")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? StoreDir { get; init; }

    [JsonPropertyName("password_secret_transport")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public CredentialSecretTransportPayload? PasswordSecretTransport { get; init; }
}

public sealed record CredentialSecretTransportPayload
{
    [JsonPropertyName("transport_kind")]
    public string TransportKind { get; init; } = "windows_named_pipe_utf8_v1";

    [JsonPropertyName("pipe_name")]
    public string PipeName { get; init; } = "";

    [JsonPropertyName("secret_nonce")]
    public string SecretNonce { get; init; } = "";

    [JsonPropertyName("timeout_ms")]
    public ulong TimeoutMs { get; init; } = 30_000;
}

public sealed record EnrollFaceTemplatePayload
{
    [JsonPropertyName("install_dir")]
    public string InstallDir { get; init; } = "";

    [JsonPropertyName("camera_id")]
    public string CameraId { get; init; } = "opencv-index:0";

    [JsonPropertyName("user_id")]
    public string UserId { get; init; } = "dev-user";

    [JsonPropertyName("output_relative_dir")]
    public string OutputRelativeDir { get; init; } = "face-enrollment";

    [JsonPropertyName("output_template_relative_path")]
    public string OutputTemplateRelativePath { get; init; } = "selected_templates.json";

    [JsonPropertyName("accepted_frames_per_step")]
    public uint AcceptedFramesPerStep { get; init; } = 6;

    [JsonPropertyName("max_wait_frames_per_step")]
    public uint MaxWaitFramesPerStep { get; init; } = 180;

    [JsonPropertyName("max_frames_per_step")]
    public uint MaxFramesPerStep { get; init; } = 180;

    [JsonPropertyName("pose_ready_consecutive")]
    public uint PoseReadyConsecutive { get; init; } = 3;

    [JsonPropertyName("pose_ready_min_fit")]
    public float PoseReadyMinFit { get; init; } = 0.25f;

    [JsonPropertyName("frame_delay_ms")]
    public uint FrameDelayMs { get; init; } = 60;

    [JsonPropertyName("allow_partial_enrollment")]
    public bool AllowPartialEnrollment { get; init; }

    [JsonPropertyName("save_debug_images")]
    public bool SaveDebugImages { get; init; }
}

public sealed record RunAuthSelfTestPayload
{
    [JsonPropertyName("install_dir")]
    public string InstallDir { get; init; } = "";

    [JsonPropertyName("session_id")]
    public string SessionId { get; init; } = "setup-auth-self-test";

    [JsonPropertyName("require_credential_ready")]
    public bool RequireCredentialReady { get; init; } = true;
}

public sealed record InstallSystemComponentsPayload
{
    [JsonPropertyName("install_dir")]
    public string InstallDir { get; init; } = "";

    [JsonPropertyName("start_service")]
    public bool StartService { get; init; } = true;

    [JsonPropertyName("configure_local_camera_auth")]
    public bool ConfigureLocalCameraAuth { get; init; }

    [JsonPropertyName("service_binary_relative_path")]
    public string ServiceBinaryRelativePath { get; init; } = "win_service.exe";

    [JsonPropertyName("control_tray_relative_path")]
    public string ControlTrayRelativePath { get; init; } = "control_tray.exe";

    [JsonPropertyName("provider_binary_relative_path")]
    public string ProviderBinaryRelativePath { get; init; } = @"provider\windows_provider.dll";

    [JsonPropertyName("face_template_relative_path")]
    public string FaceTemplateRelativePath { get; init; } = "selected_templates.json";

    [JsonPropertyName("yunet_model_relative_path")]
    public string YuNetModelRelativePath { get; init; } = @"models\face_detection_yunet_2023mar.onnx";

    [JsonPropertyName("sface_model_relative_path")]
    public string SFaceModelRelativePath { get; init; } = @"models\ghostfacenet_v1_stride2.onnx";

    [JsonPropertyName("minifasnet_model_relative_path")]
    public string MiniFasNetModelRelativePath { get; init; } = @"models\minifasnet_v2.onnx";

    [JsonPropertyName("camera_id")]
    public string CameraId { get; init; } = "opencv-index:0";

    [JsonPropertyName("match_threshold")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public float? MatchThreshold { get; init; }

    [JsonPropertyName("required_consecutive_match_count")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public uint? RequiredConsecutiveMatchCount { get; init; }

    [JsonPropertyName("provider_mode")]
    public ProviderModePayload ProviderMode { get; init; } = new();
}

public sealed record ProviderModePayload
{
    [JsonPropertyName("wake_auth_source")]
    public string WakeAuthSource { get; init; } = "local-camera";

    [JsonPropertyName("tile_visibility")]
    public string TileVisibility { get; init; } = "hidden-until-ready";

    [JsonPropertyName("auto_wake_on_advise")]
    public bool AutoWakeOnAdvise { get; init; } = true;
}

public sealed record UninstallPayload(
    [property: JsonPropertyName("preserve_data")] bool PreserveData = false,
    [property: JsonPropertyName("stop_service_first")] bool StopServiceFirst = true);
