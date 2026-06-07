using Microsoft.UI.Xaml;

namespace WinFaceUnlock.Setup.App;

public partial class App : Application
{
    private Window? _window;

    public App()
    {
        LogStartup("App constructor started.");
        UnhandledException += OnUnhandledException;
        try
        {
            InitializeComponent();
            LogStartup("App XAML initialized.");
        }
        catch (Exception error)
        {
            LogStartup($"App XAML initialization failed: {error}");
            throw;
        }
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        LogStartup("OnLaunched started.");
        try
        {
            _window = new MainWindow();
            LogStartup("MainWindow constructed.");
            _window.Activate();
            LogStartup("MainWindow activated.");
        }
        catch (Exception error)
        {
            LogStartup($"OnLaunched failed: {error}");
            throw;
        }
    }

    private static void OnUnhandledException(object sender, Microsoft.UI.Xaml.UnhandledExceptionEventArgs args)
    {
        LogStartup($"Unhandled XAML exception: {args.Exception}");
    }

    private static void LogStartup(string message)
    {
        try
        {
            var logDir = Path.Combine(Path.GetTempPath(), "WinFaceUnlockSetupApp");
            Directory.CreateDirectory(logDir);
            File.AppendAllText(
                Path.Combine(logDir, "startup.log"),
                $"{DateTimeOffset.Now:O} {message}{Environment.NewLine}");
        }
        catch
        {
        }
    }
}
