using System.Diagnostics;
using System.Text.Json;

namespace WinFaceUnlock.Setup.BackendClient;

public sealed class SetupBackendClient
{
    public const int ProtocolVersion = 1;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = false
    };

    private readonly string _backendExePath;

    public SetupBackendClient(string backendExePath)
    {
        if (string.IsNullOrWhiteSpace(backendExePath))
        {
            throw new ArgumentException("Backend executable path is required.", nameof(backendExePath));
        }

        _backendExePath = backendExePath;
    }

    public async Task<SetupResponseEnvelope> SendAsync(
        string operation,
        object payload,
        CancellationToken cancellationToken)
    {
        if (!File.Exists(_backendExePath))
        {
            throw new FileNotFoundException("Setup backend executable was not found.", _backendExePath);
        }

        var request = new SetupRequestEnvelope(
            ProtocolVersion,
            $"setup-ui-{Guid.NewGuid():N}",
            operation,
            payload);
        var requestJson = JsonSerializer.Serialize(request, JsonOptions);

        using var process = StartBackendProcess();
        await process.StandardInput.WriteAsync(requestJson.AsMemory(), cancellationToken);
        await process.StandardInput.FlushAsync(cancellationToken);
        process.StandardInput.Close();

        var stdoutTask = process.StandardOutput.ReadToEndAsync(cancellationToken);
        var stderrTask = process.StandardError.ReadToEndAsync(cancellationToken);
        await process.WaitForExitAsync(cancellationToken);

        var stdout = await stdoutTask;
        var stderr = await stderrTask;
        var responseJson = LastJsonObjectLine(stdout);
        if (responseJson is null)
        {
            throw new SetupBackendException(
                "Setup backend did not return a JSON response.",
                process.ExitCode,
                SafeOutputTail(stdout),
                SafeOutputTail(stderr));
        }

        var response = JsonSerializer.Deserialize<SetupResponseEnvelope>(responseJson, JsonOptions)
            ?? throw new SetupBackendException(
                "Setup backend returned an empty response.",
                process.ExitCode,
                SafeOutputTail(stdout),
                SafeOutputTail(stderr));

        if (process.ExitCode != 0 && response.Succeeded)
        {
            throw new SetupBackendException(
                "Setup backend exited with a non-zero code after reporting success.",
                process.ExitCode,
                response.Message,
                SafeOutputTail(stderr));
        }

        return response;
    }

    private Process StartBackendProcess()
    {
        var startInfo = new ProcessStartInfo
        {
            FileName = _backendExePath,
            UseShellExecute = false,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true,
            WorkingDirectory = Path.GetDirectoryName(_backendExePath) ?? Environment.CurrentDirectory
        };
        startInfo.ArgumentList.Add("setup-backend");

        return Process.Start(startInfo)
            ?? throw new InvalidOperationException("Failed to start setup backend process.");
    }

    private static string? LastJsonObjectLine(string text)
    {
        return text
            .Split(new[] { "\r\n", "\n" }, StringSplitOptions.RemoveEmptyEntries)
            .Reverse()
            .Select(line => line.Trim())
            .FirstOrDefault(line => line.StartsWith('{') && line.EndsWith('}'));
    }

    private static string SafeOutputTail(string text)
    {
        const int maxChars = 4000;
        if (text.Length <= maxChars)
        {
            return text;
        }

        return text[^maxChars..];
    }
}

public sealed class SetupBackendException : Exception
{
    public SetupBackendException(
        string message,
        int exitCode,
        string stdoutTail,
        string stderrTail)
        : base(message)
    {
        ExitCode = exitCode;
        StdoutTail = stdoutTail;
        StderrTail = stderrTail;
    }

    public int ExitCode { get; }
    public string StdoutTail { get; }
    public string StderrTail { get; }
}
