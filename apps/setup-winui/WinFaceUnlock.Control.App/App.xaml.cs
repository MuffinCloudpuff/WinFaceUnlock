using Microsoft.UI.Xaml;

namespace WinFaceUnlock.Control.App;

public partial class App : Application
{
    private Window? _window;

    public App()
    {
        UnhandledException += OnUnhandledException;
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        _window = new MainWindow();
        _window.Activate();
    }

    private static void OnUnhandledException(object sender, Microsoft.UI.Xaml.UnhandledExceptionEventArgs args)
    {
        WriteLog($"Unhandled XAML exception: {args.Exception}");
    }

    internal static void WriteLog(string message)
    {
        try
        {
            var logDir = Path.Combine(Path.GetTempPath(), "WinFaceUnlockControlApp");
            Directory.CreateDirectory(logDir);
            File.AppendAllText(
                Path.Combine(logDir, "control.log"),
                $"{DateTimeOffset.Now:O} {message}{Environment.NewLine}");
        }
        catch
        {
        }
    }
}
