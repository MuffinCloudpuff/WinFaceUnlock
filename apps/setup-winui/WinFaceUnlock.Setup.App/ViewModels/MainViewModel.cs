using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using Microsoft.UI.Xaml;
using WinFaceUnlock.Setup.BackendClient;
using WinRT.Interop;

namespace WinFaceUnlock.Setup.App.ViewModels;

public enum SetupWizardPage
{
    Welcome,
    Disclaimer,
    InstallLocation,
    Installing,
    Complete
}

public sealed class MainViewModel : NotifyObject
{
    private const string ProductInstallDirectoryName = "WinFaceUnlock";
    private const string SetupLogDirectoryName = "WinFaceUnlockSetupApp";
    private readonly Window _window;
    private readonly Action _closeWindow;
    private readonly CancellationTokenSource _shutdown = new();
    private string _installDir = @"C:\Program Files\WinFaceUnlock";
    private readonly string _payloadRootDir;
    private bool _useChinese = true;
    private SetupWizardPage _currentPage = SetupWizardPage.Welcome;
    private bool _disclaimerAccepted;
    private bool _isInstalling;
    private bool _installSucceeded;
    private string _statusMessage;
    private string _errorMessage = "";
    private string _errorDetails = "";
    private double _installProgress;

    public MainViewModel(Window window, Action closeWindow)
    {
        _window = window;
        _closeWindow = closeWindow;
        _payloadRootDir = DiscoverPayloadRoot();
        _statusMessage = Text("准备安装。", "Ready to install.");

        Steps = new ObservableCollection<SetupStepViewModel>
        {
            new("inspect", StepTitle("inspect")),
            new("preflight", StepTitle("preflight")),
            new("stage", StepTitle("stage")),
            new("install", StepTitle("install")),
            new("cleanup", StepTitle("cleanup"))
        };

        ToggleLanguageCommand = new AsyncRelayCommand(ToggleLanguageAsync);
        BackCommand = new AsyncRelayCommand(BackAsync, CanGoBack);
        NextCommand = new AsyncRelayCommand(NextAsync, CanGoNext);
        BrowseInstallDirCommand = new AsyncRelayCommand(PickInstallFolderAsync, CanBrowse);
        FinishCommand = new AsyncRelayCommand(FinishAsync, CanFinish);
    }

    public ObservableCollection<SetupStepViewModel> Steps { get; }

    public AsyncRelayCommand ToggleLanguageCommand { get; }
    public AsyncRelayCommand BackCommand { get; }
    public AsyncRelayCommand NextCommand { get; }
    public AsyncRelayCommand BrowseInstallDirCommand { get; }
    public AsyncRelayCommand FinishCommand { get; }

    public string InstallDir
    {
        get => _installDir;
        set
        {
            if (SetProperty(ref _installDir, value))
            {
                RaiseCommandStates();
            }
        }
    }

    public SetupWizardPage CurrentPage
    {
        get => _currentPage;
        set
        {
            if (SetProperty(ref _currentPage, value))
            {
                NotifyPageVisibility();
                OnPropertyChanged(nameof(CurrentStepName));
                OnPropertyChanged(nameof(PrimaryActionText));
                RaiseCommandStates();
            }
        }
    }

    public bool DisclaimerAccepted
    {
        get => _disclaimerAccepted;
        set
        {
            if (SetProperty(ref _disclaimerAccepted, value))
            {
                RaiseCommandStates();
            }
        }
    }

    public bool IsInstalling
    {
        get => _isInstalling;
        set
        {
            if (SetProperty(ref _isInstalling, value))
            {
                OnPropertyChanged(nameof(IsNotInstalling));
                OnPropertyChanged(nameof(InstallProgressIsIndeterminate));
                RaiseCommandStates();
            }
        }
    }

    public bool IsNotInstalling => !IsInstalling;
    public bool InstallProgressIsIndeterminate => IsInstalling && InstallProgress <= 0;

    public bool InstallSucceeded
    {
        get => _installSucceeded;
        set
        {
            if (SetProperty(ref _installSucceeded, value))
            {
                OnPropertyChanged(nameof(CompleteTitle));
                OnPropertyChanged(nameof(CompleteBody));
            }
        }
    }

    public double InstallProgress
    {
        get => _installProgress;
        set
        {
            if (SetProperty(ref _installProgress, value))
            {
                OnPropertyChanged(nameof(InstallProgressIsIndeterminate));
            }
        }
    }

    public string StatusMessage
    {
        get => _statusMessage;
        set => SetProperty(ref _statusMessage, value);
    }

    public string ErrorMessage
    {
        get => _errorMessage;
        set
        {
            if (SetProperty(ref _errorMessage, value))
            {
                OnPropertyChanged(nameof(ErrorVisibility));
            }
        }
    }

    public string ErrorDetails
    {
        get => _errorDetails;
        set => SetProperty(ref _errorDetails, value);
    }

    public string AppTitle => Text("WinFaceUnlock 安装程序", "WinFaceUnlock Setup");
    public string LanguageToggleText => _useChinese ? "English" : "中文";
    public string ProductName => "WinFaceUnlock";
    public string CurrentStepName => CurrentPage switch
    {
        SetupWizardPage.Welcome => Text("欢迎", "Welcome"),
        SetupWizardPage.Disclaimer => Text("免责声明", "Disclaimer"),
        SetupWizardPage.InstallLocation => Text("安装位置", "Install location"),
        SetupWizardPage.Installing => Text("安装", "Install"),
        SetupWizardPage.Complete => Text("完成", "Complete"),
        _ => ""
    };

    public string WelcomeTitle => Text("WinFaceUnlock", "WinFaceUnlock");
    public string WelcomeSubtitle => Text(
        "安装 Windows 本地人脸解锁组件。",
        "Install the local Windows face unlock components.");
    public string WelcomeBody => Text(
        "安装程序会复制必要文件，注册 Windows 服务和 Credential Provider。人脸录入、账号绑定和摄像头设置将在安装完成后由正式配置面板处理。",
        "Setup will copy required files and register the Windows Service and Credential Provider. Face enrollment, account binding, and camera settings are handled later in the main configuration panel.");

    public string DisclaimerTitle => Text("免责声明", "Disclaimer");
    public string DisclaimerBody => Text(
        "本项目涉及 Windows 登录、锁屏、Credential Provider 和本地系统服务等极其敏感的系统行为。安装前请务必了解：\n\n1. 高风险操作：错误的安装、配置或二次开发可能导致系统无法正常登录。\n2. 测试建议：强烈建议先在 VMware、Hyper-V 等虚拟机环境中调试和验证。\n3. 保留后路：请务必保留可用的 Windows PIN、密码、管理员账号或其他系统恢复手段。\n4. 环境警告：请勿在生产环境、重要工作电脑或保存关键数据的机器上直接实验未经验证的版本。\n5. 免责条款：作者不对因使用、修改、分发或二次开发本软件导致的任何数据丢失、系统崩溃、无法登录、安全漏洞或其他损失承担责任。",
        "This project touches extremely sensitive Windows behaviors, such as sign-in, lock screen integration, Credential Provider, and local system services. Before continuing, confirm that you understand:\n\n1. High-Risk Operations: Incorrect installation, configuration, or modification may prevent the system from signing in normally.\n2. Testing Recommendation: Debugging and validation in VMware, Hyper-V, or another virtual machine environment is strongly recommended first.\n3. Keep Backups: Please ensure you have a working Windows PIN, password, administrator account, or system recovery method available.\n4. Environment Warning: Do not test unverified builds directly on production machines, important work computers, or devices storing critical data.\n5. Limitation of Liability: The author is not responsible for any data loss, system crash, sign-in failure, security issue, or other damage caused by using, modifying, distributing, or developing this software.");
    public string AcceptDisclaimerText => Text("我已阅读并理解上述说明", "I have read and understand this notice");

    public string InstallLocationTitle => Text("选择安装位置", "Choose Install Location");
    public string InstallLocationBody => Text(
        "选择 WinFaceUnlock 程序文件的安装目录。",
        "Choose where WinFaceUnlock program files will be installed.");
    public string InstallDirectoryLabel => Text("安装目录", "Install directory");
    public string InstallDirPlaceholder => @"C:\Program Files\WinFaceUnlock";
    public string BrowseButtonText => Text("浏览", "Browse");

    public string InstallingTitle => Text("正在安装", "Installing");
    public string InstallingBody => Text(
        "请等待安装完成。当前运行中的临时安装目录会在点击完成并退出后由启动器删除。",
        "Please wait while setup completes. The temporary setup directory currently running this app is deleted by the bootstrapper after you click Finish and setup exits.");

    public string CompleteTitle => InstallSucceeded
        ? Text("安装完成", "Installation Complete")
        : Text("安装未完成", "Installation Did Not Complete");
    public string CompleteBody => InstallSucceeded
        ? Text(
            "WinFaceUnlock 已安装。点击完成后安装程序会退出，并尝试启动正式配置面板。",
            "WinFaceUnlock has been installed. Click Finish to exit setup and try to launch the main configuration panel.")
        : Text(
            "安装过程中出现问题。下面会显示具体失败原因；修复后请重新运行安装程序。",
            "Setup encountered a problem. The specific failure reason is shown below; rerun setup after remediation.");
    public string ErrorTitle => Text("失败原因", "Failure reason");

    public string BackButtonText => Text("上一步", "Back");
    public string NextButtonText => Text("下一步", "Next");
    public string InstallButtonText => Text("安装", "Install");
    public string FinishButtonText => Text("完成", "Finish");
    public string CancelButtonText => Text("取消", "Cancel");
    public string PrimaryActionText => CurrentPage == SetupWizardPage.InstallLocation
        ? InstallButtonText
        : NextButtonText;

    public Visibility WelcomeVisibility => PageVisibility(SetupWizardPage.Welcome);
    public Visibility DisclaimerVisibility => PageVisibility(SetupWizardPage.Disclaimer);
    public Visibility InstallLocationVisibility => PageVisibility(SetupWizardPage.InstallLocation);
    public Visibility InstallingVisibility => PageVisibility(SetupWizardPage.Installing);
    public Visibility CompleteVisibility => PageVisibility(SetupWizardPage.Complete);
    public Visibility WizardNavigationVisibility => CurrentPage == SetupWizardPage.Installing
        || CurrentPage == SetupWizardPage.Complete
            ? Visibility.Collapsed
            : Visibility.Visible;
    public Visibility FinishVisibility => CurrentPage == SetupWizardPage.Complete
        ? Visibility.Visible
        : Visibility.Collapsed;
    public Visibility ErrorVisibility => string.IsNullOrWhiteSpace(ErrorMessage)
        ? Visibility.Collapsed
        : Visibility.Visible;

    private string BackendPath => Path.Combine(_payloadRootDir, "installer_cli.exe");

    private SetupFlowCoordinator CreateFlow()
    {
        return new SetupFlowCoordinator(new SetupBackendClient(BackendPath));
    }

    private Task ToggleLanguageAsync()
    {
        _useChinese = !_useChinese;
        if (StringComparer.Ordinal.Equals(StatusMessage, "Ready to install.")
            || StringComparer.Ordinal.Equals(StatusMessage, "准备安装。"))
        {
            StatusMessage = Text("准备安装。", "Ready to install.");
        }

        RefreshLocalizedText();
        return Task.CompletedTask;
    }

    private Task BackAsync()
    {
        CurrentPage = CurrentPage switch
        {
            SetupWizardPage.Disclaimer => SetupWizardPage.Welcome,
            SetupWizardPage.InstallLocation => SetupWizardPage.Disclaimer,
            _ => CurrentPage
        };
        return Task.CompletedTask;
    }

    private async Task NextAsync()
    {
        switch (CurrentPage)
        {
            case SetupWizardPage.Welcome:
                CurrentPage = SetupWizardPage.Disclaimer;
                break;
            case SetupWizardPage.Disclaimer:
                CurrentPage = SetupWizardPage.InstallLocation;
                break;
            case SetupWizardPage.InstallLocation:
                await RunInstallAsync();
                break;
        }
    }

    private async Task RunInstallAsync()
    {
        if (!File.Exists(BackendPath))
        {
            StatusMessage = Text(
                "安装后端不存在，请重新构建安装包。",
                "Setup backend is missing. Rebuild the setup package.");
            return;
        }

        InstallDir = NormalizeInstallDirectory(InstallDir);
        ResetInstallProgress();
        ClearFailure();
        WriteInstallLog($"Install started. install_dir={InstallDir}; payload_root={_payloadRootDir}");
        CurrentPage = SetupWizardPage.Installing;
        IsInstalling = true;
        InstallSucceeded = false;

        var flow = CreateFlow();
        var plan = flow.CreateInstallPlan(new SetupInstallPlanOptions
        {
            InstallDir = InstallDir,
            PayloadRootDir = _payloadRootDir
        });

        var totalSteps = plan.Count + 1;
        var completedSteps = 0;
        foreach (var planStep in plan)
        {
            var response = await RunSetupStepAsync(
                planStep.StepKey,
                RunningMessage(planStep.RunningMessageKey),
                planStep.ExecuteAsync);
            if (response is null || !response.Succeeded)
            {
                IsInstalling = false;
                InstallSucceeded = false;
                CurrentPage = SetupWizardPage.Complete;
                return;
            }

            completedSteps += 1;
            InstallProgress = completedSteps * 100.0 / totalSteps;
        }

        await RunCleanupStepAsync();
        InstallProgress = 100;
        IsInstalling = false;
        InstallSucceeded = true;
        StatusMessage = Text("安装完成。", "Installation complete.");
        WriteInstallLog("Install completed successfully.");
        CurrentPage = SetupWizardPage.Complete;
    }

    private async Task<SetupResponseEnvelope?> RunSetupStepAsync(
        string stepKey,
        string runningMessage,
        Func<CancellationToken, Task<SetupResponseEnvelope>> operation)
    {
        var step = Step(stepKey);
        step.State = SetupStepState.Running;
        StatusMessage = runningMessage;
        WriteInstallLog($"Step started. step={stepKey}; message={runningMessage}");

        try
        {
            var response = await operation(_shutdown.Token);
            step.State = response.Succeeded ? SetupStepState.Succeeded : SetupStepState.Failed;
            StatusMessage = response.Succeeded ? response.Message : $"{response.OperationStatus}: {response.Message}";
            AddSafeDetailStatus(response.SafeDetails);
            WriteInstallLog(
                $"Step finished. step={stepKey}; status={response.OperationStatus}; message={response.Message}; next={response.NextRecommendedAction ?? ""}; safe_details={SafeRawJson(response.SafeDetails)}");
            if (!response.Succeeded)
            {
                RecordFailure(stepKey, response);
            }
            return response;
        }
        catch (Exception error)
        {
            step.State = SetupStepState.Failed;
            StatusMessage = error.Message;
            RecordFailure(stepKey, error);
            return null;
        }
    }

    private Task RunCleanupStepAsync()
    {
        var step = Step("cleanup");
        step.State = SetupStepState.Running;
        StatusMessage = Text(
            "已安排退出后清理临时安装目录",
            "Temporary setup directory will be cleaned after setup exits");
        step.State = SetupStepState.Succeeded;
        return Task.CompletedTask;
    }

    private Task FinishAsync()
    {
        if (InstallSucceeded)
        {
            TryLaunchInstalledTray();
            TryLaunchInstalledControlPanel();
        }

        _closeWindow();
        return Task.CompletedTask;
    }

    private void TryLaunchInstalledTray()
    {
        var trayPath = Path.Combine(InstallDir, "control_tray.exe");
        if (File.Exists(trayPath))
        {
            TryStartInstalledProcess(trayPath);
        }
    }

    private void TryLaunchInstalledControlPanel()
    {
        var candidates = new[]
        {
            Path.Combine(InstallDir, "WinFaceUnlock.exe"),
            Path.Combine(InstallDir, "WinFaceUnlock.Config.exe"),
            Path.Combine(InstallDir, "WinFaceUnlock.UI.exe")
        };

        var appPath = candidates.FirstOrDefault(File.Exists);
        if (appPath is null)
        {
            return;
        }

        TryStartInstalledProcess(appPath);
    }

    private static void TryStartInstalledProcess(string appPath)
    {
        try
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = appPath,
                WorkingDirectory = Path.GetDirectoryName(appPath) ?? Environment.CurrentDirectory,
                UseShellExecute = true
            });
        }
        catch
        {
        }
    }

    private Task PickInstallFolderAsync()
    {
        var selectedPath = ModernFolderPicker.PickFolder(
            WindowNative.GetWindowHandle(_window),
            InstallDirectoryLabel,
            ResolveFolderPickerInitialDirectory(InstallDir));
        if (!string.IsNullOrWhiteSpace(selectedPath))
        {
            InstallDir = NormalizeInstallDirectory(selectedPath);
        }

        return Task.CompletedTask;
    }

    private static string NormalizeInstallDirectory(string selectedPath)
    {
        if (string.IsNullOrWhiteSpace(selectedPath))
        {
            return selectedPath;
        }

        var expandedPath = Environment.ExpandEnvironmentVariables(selectedPath.Trim());
        var fullPath = Path.GetFullPath(expandedPath);
        var rootPath = Path.GetPathRoot(fullPath);
        var installPath = StringComparer.OrdinalIgnoreCase.Equals(fullPath, rootPath)
            ? fullPath
            : fullPath.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);

        var leafName = Path.GetFileName(installPath);
        return StringComparer.OrdinalIgnoreCase.Equals(leafName, ProductInstallDirectoryName)
            ? installPath
            : Path.Combine(installPath, ProductInstallDirectoryName);
    }

    private static string ResolveFolderPickerInitialDirectory(string installDirectory)
    {
        if (Directory.Exists(installDirectory))
        {
            return installDirectory;
        }

        try
        {
            var normalizedInstallDirectory = NormalizeInstallDirectory(installDirectory);
            var parentDirectory = Directory.GetParent(normalizedInstallDirectory)?.FullName;
            if (!string.IsNullOrWhiteSpace(parentDirectory) && Directory.Exists(parentDirectory))
            {
                return parentDirectory;
            }
        }
        catch
        {
        }

        return Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
    }

    private string RunningMessage(string key)
    {
        return key switch
        {
            "inspect_package" => Text("正在检查安装包", "Inspecting package"),
            "source_preflight" => Text("正在检查安装条件", "Checking setup prerequisites"),
            "stage_payload" => Text("正在复制安装文件", "Copying setup files"),
            "staged_preflight" => Text("正在验证已复制文件", "Verifying copied files"),
            "install_components" => Text("正在安装系统组件", "Installing system components"),
            _ => key
        };
    }

    private void AddSafeDetailStatus(JsonElement safeDetails)
    {
        if (safeDetails.ValueKind != JsonValueKind.Object)
        {
            return;
        }

        if (!safeDetails.TryGetProperty("checks", out var checksElement)
            || checksElement.ValueKind != JsonValueKind.Array)
        {
            return;
        }

        foreach (var checkElement in checksElement.EnumerateArray())
        {
            var status = ReadStringProperty(checkElement, "status");
            if (!StringComparer.Ordinal.Equals(status, "failed"))
            {
                continue;
            }

            var message = ReadStringProperty(checkElement, "message");
            if (!string.IsNullOrWhiteSpace(message))
            {
                StatusMessage = message;
                return;
            }
        }
    }

    private void ResetInstallProgress()
    {
        InstallProgress = 0;
        foreach (var step in Steps)
        {
            step.State = SetupStepState.Pending;
        }
    }

    private void ClearFailure()
    {
        ErrorMessage = "";
        ErrorDetails = "";
    }

    private void RecordFailure(string stepKey, SetupResponseEnvelope response)
    {
        ErrorMessage = $"{StepTitle(stepKey)}: {StatusMessage}";
        ErrorDetails = string.Join(
            Environment.NewLine,
            new[]
            {
                $"operation={response.Operation}",
                $"status={response.OperationStatus}",
                string.IsNullOrWhiteSpace(response.NextRecommendedAction)
                    ? ""
                    : $"next_action={response.NextRecommendedAction}",
                $"safe_details={SafeRawJson(response.SafeDetails)}"
            }.Where(line => !string.IsNullOrWhiteSpace(line)));
        WriteInstallLog($"Failure recorded. step={stepKey}; {ErrorMessage}; {ErrorDetails}");
    }

    private void RecordFailure(string stepKey, Exception error)
    {
        ErrorMessage = $"{StepTitle(stepKey)}: {error.Message}";
        ErrorDetails = error is SetupBackendException backendError
            ? string.Join(
                Environment.NewLine,
                new[]
                {
                    backendError.DiagnosticDetails(),
                    backendError.ToString()
                }.Where(part => !string.IsNullOrWhiteSpace(part)))
            : error.ToString();
        WriteInstallLog($"Exception recorded. step={stepKey}; {ErrorDetails}");
    }

    private SetupStepViewModel Step(string key)
    {
        return Steps.First(step => step.Key == key);
    }

    private string StepTitle(string key)
    {
        return key switch
        {
            "inspect" => Text("检查安装包", "Inspect package"),
            "preflight" => Text("检查条件", "Check prerequisites"),
            "stage" => Text("复制文件", "Copy files"),
            "install" => Text("安装组件", "Install components"),
            "cleanup" => Text("清理临时文件", "Clean temporary files"),
            _ => key
        };
    }

    private static string? ReadStringProperty(JsonElement element, string propertyName)
    {
        if (!element.TryGetProperty(propertyName, out var value) || value.ValueKind != JsonValueKind.String)
        {
            return null;
        }

        return value.GetString();
    }

    private bool CanGoBack()
    {
        return IsNotInstalling
            && (CurrentPage == SetupWizardPage.Disclaimer
                || CurrentPage == SetupWizardPage.InstallLocation);
    }

    private bool CanGoNext()
    {
        return IsNotInstalling
            && CurrentPage != SetupWizardPage.Complete
            && CurrentPage switch
            {
                SetupWizardPage.Disclaimer => DisclaimerAccepted,
                SetupWizardPage.InstallLocation => !string.IsNullOrWhiteSpace(InstallDir),
                _ => true
            };
    }

    private bool CanBrowse()
    {
        return IsNotInstalling && CurrentPage == SetupWizardPage.InstallLocation;
    }

    private bool CanFinish()
    {
        return IsNotInstalling && CurrentPage == SetupWizardPage.Complete;
    }

    private void RaiseCommandStates()
    {
        BackCommand.RaiseCanExecuteChanged();
        NextCommand.RaiseCanExecuteChanged();
        BrowseInstallDirCommand.RaiseCanExecuteChanged();
        FinishCommand.RaiseCanExecuteChanged();
    }

    private void RefreshLocalizedText()
    {
        foreach (var step in Steps)
        {
            step.Title = StepTitle(step.Key);
        }

        foreach (var propertyName in LocalizedPropertyNames)
        {
            OnPropertyChanged(propertyName);
        }

        OnPropertyChanged(nameof(CurrentStepName));
        OnPropertyChanged(nameof(PrimaryActionText));
        NotifyPageVisibility();
    }

    private void NotifyPageVisibility()
    {
        OnPropertyChanged(nameof(WelcomeVisibility));
        OnPropertyChanged(nameof(DisclaimerVisibility));
        OnPropertyChanged(nameof(InstallLocationVisibility));
        OnPropertyChanged(nameof(InstallingVisibility));
        OnPropertyChanged(nameof(CompleteVisibility));
        OnPropertyChanged(nameof(WizardNavigationVisibility));
        OnPropertyChanged(nameof(FinishVisibility));
    }

    private Visibility PageVisibility(SetupWizardPage page)
    {
        return CurrentPage == page ? Visibility.Visible : Visibility.Collapsed;
    }

    private string Text(string chinese, string english)
    {
        return _useChinese ? chinese : english;
    }

    private static string SafeRawJson(JsonElement element)
    {
        return element.ValueKind == JsonValueKind.Undefined ? "{}" : element.GetRawText();
    }

    private static void WriteInstallLog(string message)
    {
        try
        {
            var logDir = Path.Combine(Path.GetTempPath(), SetupLogDirectoryName);
            Directory.CreateDirectory(logDir);
            File.AppendAllText(
                Path.Combine(logDir, "install.log"),
                $"{DateTimeOffset.Now:O} {message}{Environment.NewLine}");
        }
        catch
        {
        }
    }

    private static readonly string[] LocalizedPropertyNames =
    [
        nameof(AppTitle),
        nameof(LanguageToggleText),
        nameof(CurrentStepName),
        nameof(WelcomeTitle),
        nameof(WelcomeSubtitle),
        nameof(WelcomeBody),
        nameof(DisclaimerTitle),
        nameof(DisclaimerBody),
        nameof(AcceptDisclaimerText),
        nameof(InstallLocationTitle),
        nameof(InstallLocationBody),
        nameof(InstallDirectoryLabel),
        nameof(InstallDirPlaceholder),
        nameof(BrowseButtonText),
        nameof(InstallingTitle),
        nameof(InstallingBody),
        nameof(CompleteTitle),
        nameof(CompleteBody),
        nameof(ErrorTitle),
        nameof(BackButtonText),
        nameof(NextButtonText),
        nameof(InstallButtonText),
        nameof(FinishButtonText),
        nameof(CancelButtonText),
        nameof(PrimaryActionText),
    ];

    private static string DiscoverPayloadRoot()
    {
        var baseDir = AppContext.BaseDirectory;
        if (File.Exists(Path.Combine(baseDir, "winfaceunlock-payload.json")))
        {
            return baseDir;
        }

        var bundledPayload = Path.GetFullPath(Path.Combine(baseDir, @"..\payload"));
        if (File.Exists(Path.Combine(bundledPayload, "winfaceunlock-payload.json")))
        {
            return bundledPayload;
        }

        var repoPayload = Path.GetFullPath(Path.Combine(baseDir, @"..\..\..\..\..\target\setup-payload"));
        return Directory.Exists(repoPayload) ? repoPayload : baseDir;
    }

    private static class ModernFolderPicker
    {
        private static readonly Guid ShellItemId = new("43826D1E-E718-42EE-BC55-A1E261C37BFE");
        private const int ErrorCancelled = unchecked((int)0x800704C7);

        public static string? PickFolder(IntPtr ownerHandle, string title, string initialDirectory)
        {
            IFileDialog? dialog = null;
            IShellItem? initialFolder = null;
            IShellItem? selectedItem = null;
            try
            {
                dialog = (IFileDialog)(object)new FileOpenDialog();
                dialog.GetOptions(out var options);
                dialog.SetOptions(options
                    | FileOpenOptions.PickFolders
                    | FileOpenOptions.ForceFileSystem
                    | FileOpenOptions.PathMustExist);
                dialog.SetTitle(title);

                if (Directory.Exists(initialDirectory)
                    && TryCreateShellItem(initialDirectory, out initialFolder)
                    && initialFolder is not null)
                {
                    dialog.SetFolder(initialFolder);
                }

                var result = dialog.Show(ownerHandle);
                if (result == ErrorCancelled)
                {
                    return null;
                }

                Marshal.ThrowExceptionForHR(result);
                dialog.GetResult(out selectedItem);
                selectedItem.GetDisplayName(ShellItemDisplayName.FileSystemPath, out var pathPointer);
                try
                {
                    return Marshal.PtrToStringUni(pathPointer);
                }
                finally
                {
                    Marshal.FreeCoTaskMem(pathPointer);
                }
            }
            finally
            {
                ReleaseComObject(selectedItem);
                ReleaseComObject(initialFolder);
                ReleaseComObject(dialog);
            }
        }

        private static bool TryCreateShellItem(string path, out IShellItem? shellItem)
        {
            try
            {
                SHCreateItemFromParsingName(path, IntPtr.Zero, ShellItemId, out shellItem);
                return shellItem is not null;
            }
            catch
            {
                shellItem = null;
                return false;
            }
        }

        private static void ReleaseComObject(object? value)
        {
            if (value is not null && Marshal.IsComObject(value))
            {
                Marshal.ReleaseComObject(value);
            }
        }

        [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = false)]
        private static extern void SHCreateItemFromParsingName(
            string pszPath,
            IntPtr pbc,
            [MarshalAs(UnmanagedType.LPStruct)] Guid riid,
            out IShellItem ppv);

        [ComImport]
        [Guid("DC1C5A9C-E88A-4DDE-A5A1-60F82A20AEF7")]
        private sealed class FileOpenDialog;

        [ComImport]
        [Guid("42F85136-DB7E-439C-85F1-E4075D135FC8")]
        [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
        private interface IFileDialog
        {
            [PreserveSig]
            int Show(IntPtr parent);
            void SetFileTypes(uint cFileTypes, IntPtr rgFilterSpec);
            void SetFileTypeIndex(uint iFileType);
            void GetFileTypeIndex(out uint piFileType);
            void Advise(IntPtr pfde, out uint pdwCookie);
            void Unadvise(uint dwCookie);
            void SetOptions(FileOpenOptions fos);
            void GetOptions(out FileOpenOptions pfos);
            void SetDefaultFolder(IShellItem psi);
            void SetFolder(IShellItem psi);
            void GetFolder(out IShellItem ppsi);
            void GetCurrentSelection(out IShellItem ppsi);
            void SetFileName([MarshalAs(UnmanagedType.LPWStr)] string pszName);
            void GetFileName(out IntPtr pszName);
            void SetTitle([MarshalAs(UnmanagedType.LPWStr)] string pszTitle);
            void SetOkButtonLabel([MarshalAs(UnmanagedType.LPWStr)] string pszText);
            void SetFileNameLabel([MarshalAs(UnmanagedType.LPWStr)] string pszLabel);
            void GetResult(out IShellItem ppsi);
            void AddPlace(IShellItem psi, uint fdap);
            void SetDefaultExtension([MarshalAs(UnmanagedType.LPWStr)] string pszDefaultExtension);
            void Close(int hr);
            void SetClientGuid(ref Guid guid);
            void ClearClientData();
            void SetFilter(IntPtr pFilter);
        }

        [ComImport]
        [Guid("43826D1E-E718-42EE-BC55-A1E261C37BFE")]
        [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
        private interface IShellItem
        {
            void BindToHandler(IntPtr pbc, ref Guid bhid, ref Guid riid, out IntPtr ppv);
            void GetParent(out IShellItem ppsi);
            void GetDisplayName(ShellItemDisplayName sigdnName, out IntPtr ppszName);
            void GetAttributes(uint sfgaoMask, out uint psfgaoAttribs);
            void Compare(IShellItem psi, uint hint, out int piOrder);
        }

        [Flags]
        private enum FileOpenOptions : uint
        {
            PickFolders = 0x00000020,
            ForceFileSystem = 0x00000040,
            PathMustExist = 0x00000800,
        }

        private enum ShellItemDisplayName : uint
        {
            FileSystemPath = 0x80058000,
        }
    }
}
