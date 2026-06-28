using Microsoft.UI.Xaml;

namespace WinFaceUnlock.Setup.App;

public partial class App : Application
{
    private const string ValidateBackendArg = "--winfaceunlock-setup-app-validate-backend";

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

    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        LogStartup("OnLaunched started.");
        try
        {
            var commandLineArgs = Environment.GetCommandLineArgs().Skip(1).ToList();
            if (IsBackendValidation(commandLineArgs))
            {
                try
                {
                    await RunBackendValidationAsync(commandLineArgs);
                    LogStartup("Backend validation completed.");
                    Environment.Exit(0);
                }
                catch (Exception validationError)
                {
                    LogStartup($"Backend validation failed: {DescribeException(validationError)}");
                    Environment.Exit(1);
                }
                return;
            }

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

    private static bool IsBackendValidation(IReadOnlyList<string> parsedArgs)
    {
        return parsedArgs.Count > 0 && parsedArgs[0] == ValidateBackendArg;
    }

    private static async Task RunBackendValidationAsync(IReadOnlyList<string> parsedArgs)
    {
        if (parsedArgs.Count != 3)
        {
            throw new ArgumentException(
                $"{ValidateBackendArg} requires backend executable path and payload root directory.");
        }

        var backendPath = parsedArgs[1];
        var payloadRootDir = parsedArgs[2];
        var client = new BackendClient.SetupBackendClient(backendPath);
        var response = await client
            .SendAsync(
                BackendClient.SetupOperations.InspectPayload,
                new BackendClient.InspectPayloadPayload(payloadRootDir),
                CancellationToken.None);
        if (!response.Succeeded)
        {
            throw new InvalidOperationException(
                $"Backend validation inspect_payload failed: {response.OperationStatus}; {response.Message}");
        }

        return;
    }

    private static void OnUnhandledException(object sender, Microsoft.UI.Xaml.UnhandledExceptionEventArgs args)
    {
        LogStartup($"Unhandled XAML exception: {DescribeException(args.Exception)}");
    }

    private static string DescribeException(Exception error)
    {
        if (error is BackendClient.SetupBackendException backendError)
        {
            return string.Join(
                Environment.NewLine,
                new[]
                {
                    backendError.DiagnosticDetails(),
                    backendError.ToString()
                }.Where(part => !string.IsNullOrWhiteSpace(part)));
        }

        return error.ToString();
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
