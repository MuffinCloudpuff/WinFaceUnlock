namespace WinFaceUnlock.Control.App.Backends;

public interface IControlStatusProvider
{
    string SourceDescription { get; }

    Task<ControlStatusSnapshot> LoadStatusAsync(CancellationToken cancellationToken);
}
