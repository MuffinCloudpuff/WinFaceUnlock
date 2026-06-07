using System;
using System.IO.Pipes;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace WinFaceUnlock.Setup.BackendClient;

internal sealed class CredentialSecretPipe : IDisposable
{
    private const string PipeNamePrefix = "winfaceunlock-credential-";
    private const string ProtocolHeader = "WFU_SECRET_PIPE_V1";
    private readonly NamedPipeServerStream _server;

    private CredentialSecretPipe(
        string pipeName,
        string secretNonce,
        ulong timeoutMs,
        NamedPipeServerStream server)
    {
        PipeName = pipeName;
        SecretNonce = secretNonce;
        TimeoutMs = timeoutMs;
        _server = server;
    }

    public string PipeName { get; }
    public string SecretNonce { get; }
    public ulong TimeoutMs { get; }

    public CredentialSecretTransportPayload TransportPayload => new()
    {
        PipeName = PipeName,
        SecretNonce = SecretNonce,
        TimeoutMs = TimeoutMs
    };

    public static CredentialSecretPipe Create(ulong timeoutMs = 30_000)
    {
        var pipeName = $"{PipeNamePrefix}{Guid.NewGuid():N}";
        var secretNonce = Convert.ToHexString(RandomNumberGenerator.GetBytes(16)).ToLowerInvariant();
        var server = new NamedPipeServerStream(
            pipeName,
            PipeDirection.Out,
            maxNumberOfServerInstances: 1,
            PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous);

        return new CredentialSecretPipe(pipeName, secretNonce, timeoutMs, server);
    }

    public async Task WriteCredentialSecretAsync(
        string password,
        CancellationToken cancellationToken)
    {
        await _server.WaitForConnectionAsync(cancellationToken);

        var headerBytes = Encoding.UTF8.GetBytes($"{ProtocolHeader}\n{SecretNonce}\n");
        var passwordBytes = Encoding.UTF8.GetBytes(password);
        try
        {
            await _server.WriteAsync(headerBytes.AsMemory(), cancellationToken);
            await _server.WriteAsync(passwordBytes.AsMemory(), cancellationToken);
            await _server.FlushAsync(cancellationToken);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(headerBytes);
            CryptographicOperations.ZeroMemory(passwordBytes);
        }
    }

    public void Dispose()
    {
        _server.Dispose();
    }
}
