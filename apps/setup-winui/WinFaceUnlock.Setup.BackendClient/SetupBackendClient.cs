using System.Diagnostics;
using System.Text;
using System.Text.Json;

namespace WinFaceUnlock.Setup.BackendClient;

public sealed class SetupBackendClient
{
    public const int ProtocolVersion = 1;
    private static readonly Encoding Utf8NoBom = new UTF8Encoding(encoderShouldEmitUTF8Identifier: false);

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
        process.StandardInput.Write(requestJson);
        process.StandardInput.Flush();
        process.StandardInput.Close();

        var stdoutTask = process.StandardOutput.ReadToEndAsync(cancellationToken);
        var stderrTask = process.StandardError.ReadToEndAsync(cancellationToken);
        await process.WaitForExitAsync(cancellationToken)
            .ConfigureAwait(false);

        var stdout = await stdoutTask.ConfigureAwait(false);
        var stderr = await stderrTask.ConfigureAwait(false);
        var responseJson = LastJsonObjectLine(stdout);
        if (responseJson is null)
        {
            throw new SetupBackendException(
                "Setup backend did not return a JSON response.",
                _backendExePath,
                process.ExitCode,
                SafeOutputTail(stdout),
                SafeOutputTail(stderr));
        }

        var response = JsonSerializer.Deserialize<SetupResponseEnvelope>(responseJson, JsonOptions)
            ?? throw new SetupBackendException(
                "Setup backend returned an empty response.",
                _backendExePath,
                process.ExitCode,
                SafeOutputTail(stdout),
                SafeOutputTail(stderr));

        if (process.ExitCode != 0 && response.Succeeded)
        {
            throw new SetupBackendException(
                "Setup backend exited with a non-zero code after reporting success.",
                _backendExePath,
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
            StandardInputEncoding = Utf8NoBom,
            StandardOutputEncoding = Utf8NoBom,
            StandardErrorEncoding = Utf8NoBom,
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
        string backendExePath,
        int exitCode,
        string stdoutTail,
        string stderrTail)
        : base(message)
    {
        BackendExePath = backendExePath;
        ExitCode = exitCode;
        StdoutTail = stdoutTail;
        StderrTail = stderrTail;
    }

    public string BackendExePath { get; }
    public int ExitCode { get; }
    public string StdoutTail { get; }
    public string StderrTail { get; }

    public string DiagnosticDetails()
    {
        return string.Join(
            Environment.NewLine,
            new[]
            {
                $"backend_exe={BackendExePath}",
                $"exit_code={ExitCode}",
                string.IsNullOrWhiteSpace(StdoutTail) ? "" : $"stdout_tail={StdoutTail}",
                string.IsNullOrWhiteSpace(StderrTail) ? "" : $"stderr_tail={StderrTail}"
            }.Where(line => !string.IsNullOrWhiteSpace(line)));
    }
}
