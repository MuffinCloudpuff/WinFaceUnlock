namespace WinFaceUnlock.Control.App.Backends;

public sealed record ControlStatusSnapshot(
    string StatusMessage,
    ControlStatusItem Service,
    ControlStatusItem Provider,
    ControlStatusItem AuthenticationConfig,
    ControlStatusItem DataDirectory);
