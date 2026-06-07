namespace WinFaceUnlock.Control.App.Backends;

public sealed record ControlStatusItem(
    string Title,
    string Value,
    string Detail,
    bool IsHealthy);
