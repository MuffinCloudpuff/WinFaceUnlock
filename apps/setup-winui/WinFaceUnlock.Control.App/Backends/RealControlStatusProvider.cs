using System.Text.Json;
using WinFaceUnlock.Setup.BackendClient;

namespace WinFaceUnlock.Control.App.Backends;

public sealed class RealControlStatusProvider : IControlStatusProvider
{
    public string SourceDescription => "后端自动连接";

    public async Task<ControlStatusSnapshot> LoadStatusAsync(CancellationToken cancellationToken)
    {
        var backendPath = DiscoverBackendPath();
        if (backendPath is null)
        {
            return BackendUnavailable("未找到 installer_cli.exe。前端可继续独立运行，后端可用后再刷新。");
        }

        var client = new SetupBackendClient(backendPath);
        var response = await client.SendAsync(SetupOperations.GetStatus, new { }, cancellationToken);
        if (!response.Succeeded)
        {
            throw new InvalidOperationException($"{response.OperationStatus}: {response.Message}");
        }

        return MapStatus(response.SafeDetails);
    }

    private static ControlStatusSnapshot MapStatus(JsonElement details)
    {
        var service = details.GetProperty("service");
        var provider = details.GetProperty("provider");
        var serviceConfig = details.GetProperty("service_config");
        var data = details.GetProperty("data");

        var serviceInstalled = ReadBool(service, "installed");
        var serviceRunning = ReadBool(service, "running");
        var serviceState = ReadString(service, "state");
        var serviceProcessId = ReadNullableNumber(service, "process_id");

        var providerRegistered = ReadBool(provider, "registered");
        var registryConfigExists = ReadBool(serviceConfig, "registry_config_exists");
        var authMode = ReadString(serviceConfig, "auth_mode");
        var faceTemplatePath = ReadString(serviceConfig, "face_template_path");
        var programDataDir = ReadString(data, "program_data_dir");
        var programDataExists = ReadBool(data, "program_data_exists");

        return new ControlStatusSnapshot(
            "运行状态已更新。",
            new ControlStatusItem(
                "后台服务",
                serviceRunning ? "运行中" : serviceInstalled ? "已安装但未运行" : "未安装",
                serviceProcessId is null
                    ? $"状态：{serviceState}"
                    : $"状态：{serviceState}，进程 ID：{serviceProcessId}",
                serviceInstalled && serviceRunning),
            new ControlStatusItem(
                "登录组件",
                providerRegistered ? "已注册" : "未完整注册",
                $"Credential Provider：{BoolText(ReadBool(provider, "credential_provider_registered"))}；COM：{BoolText(ReadBool(provider, "com_server_registered"))}",
                providerRegistered),
            new ControlStatusItem(
                "认证配置",
                registryConfigExists ? "已写入" : "未配置",
                string.IsNullOrWhiteSpace(faceTemplatePath)
                    ? $"模式：{authMode}；尚未录入人脸模板"
                    : $"模式：{authMode}；模板：{faceTemplatePath}",
                registryConfigExists),
            new ControlStatusItem(
                "数据目录",
                programDataExists ? "可用" : "未创建",
                string.IsNullOrWhiteSpace(programDataDir) ? "未返回目录路径" : programDataDir,
                programDataExists));
    }

    private static ControlStatusSnapshot BackendUnavailable(string reason)
    {
        return new ControlStatusSnapshot(
            "后端未连接，前端仍可独立运行。",
            new ControlStatusItem(
                "后台连接",
                "未连接",
                reason,
                false),
            new ControlStatusItem(
                "登录组件",
                "等待后端",
                "后端连接后会读取 Credential Provider 注册状态。",
                false),
            new ControlStatusItem(
                "认证配置",
                "等待后端",
                "后端连接后会读取认证模式、人脸模板和账号绑定状态。",
                false),
            new ControlStatusItem(
                "数据目录",
                "等待后端",
                "后端连接后会读取 ProgramData 路径和可用性。",
                false));
    }

    private static string? DiscoverBackendPath()
    {
        var baseDir = AppContext.BaseDirectory;
        var candidates = new[]
        {
            Path.Combine(baseDir, "installer_cli.exe"),
            Path.GetFullPath(Path.Combine(baseDir, @"..\payload\installer_cli.exe")),
            Path.GetFullPath(Path.Combine(baseDir, @"..\..\..\..\..\target\setup-payload\installer_cli.exe"))
        };

        var backendPath = candidates.FirstOrDefault(File.Exists);
        return backendPath;
    }

    private static bool ReadBool(JsonElement element, string propertyName)
    {
        return element.TryGetProperty(propertyName, out var value)
            && value.ValueKind == JsonValueKind.True;
    }

    private static string ReadString(JsonElement element, string propertyName)
    {
        if (!element.TryGetProperty(propertyName, out var value))
        {
            return "";
        }

        return value.ValueKind switch
        {
            JsonValueKind.String => value.GetString() ?? "",
            JsonValueKind.Null => "",
            _ => value.ToString()
        };
    }

    private static string? ReadNullableNumber(JsonElement element, string propertyName)
    {
        if (!element.TryGetProperty(propertyName, out var value)
            || value.ValueKind == JsonValueKind.Null)
        {
            return null;
        }

        return value.ToString();
    }

    private static string BoolText(bool value)
    {
        return value ? "是" : "否";
    }
}
