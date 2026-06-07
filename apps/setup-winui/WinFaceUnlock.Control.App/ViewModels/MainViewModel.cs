using System.Windows.Input;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using WinFaceUnlock.Control.App.Backends;

namespace WinFaceUnlock.Control.App.ViewModels;

public sealed class MainViewModel : NotifyObject
{
    private static readonly Brush SelectedTabBackground = new SolidColorBrush(ColorHelper.FromArgb(230, 239, 246, 255));
    private static readonly Brush TransparentTabBackground = new SolidColorBrush(Colors.Transparent);
    private static readonly Brush SelectedTabForeground = new SolidColorBrush(ColorHelper.FromArgb(255, 0, 102, 184));
    private static readonly Brush DefaultTabForeground = new SolidColorBrush(ColorHelper.FromArgb(255, 71, 85, 105));

    private readonly CancellationTokenSource _shutdown = new();
    private readonly IControlStatusProvider _statusProvider;
    private string _activeTab = "home";
    private string _statusMessage = "正在检查后端连接...";
    private string _lastUpdated = "";
    private bool _hasError;
    private string _errorMessage = "";
    private string _pin = "";
    private bool _autoLockEnabled = true;
    private bool _intruderSnapshotEnabled = true;

    public MainViewModel(IControlStatusProvider statusProvider)
    {
        _statusProvider = statusProvider;
        RefreshCommand = new AsyncRelayCommand(RefreshAsync);
        ShowHomeCommand = new AsyncRelayCommand(() => ShowTabAsync("home"));
        ShowAccountCommand = new AsyncRelayCommand(() => ShowTabAsync("account"));
        ShowSettingsCommand = new AsyncRelayCommand(() => ShowTabAsync("settings"));
        _ = RefreshAsync();
    }

    public event EventHandler? HomeTabActivated;

    public string AppTitle => "WinFaceUnlock";
    public ICommand RefreshCommand { get; }
    public ICommand ShowHomeCommand { get; }
    public ICommand ShowAccountCommand { get; }
    public ICommand ShowSettingsCommand { get; }

    public string StatusMessage
    {
        get => _statusMessage;
        set => SetProperty(ref _statusMessage, value);
    }

    public string LastUpdated
    {
        get => _lastUpdated;
        set => SetProperty(ref _lastUpdated, value);
    }

    public bool HasError
    {
        get => _hasError;
        set
        {
            if (SetProperty(ref _hasError, value))
            {
                OnPropertyChanged(nameof(ErrorVisibility));
            }
        }
    }

    public string ErrorMessage
    {
        get => _errorMessage;
        set => SetProperty(ref _errorMessage, value);
    }

    public string Pin
    {
        get => _pin;
        set => SetProperty(ref _pin, value);
    }

    public bool AutoLockEnabled
    {
        get => _autoLockEnabled;
        set => SetProperty(ref _autoLockEnabled, value);
    }

    public bool IntruderSnapshotEnabled
    {
        get => _intruderSnapshotEnabled;
        set => SetProperty(ref _intruderSnapshotEnabled, value);
    }

    public Visibility ErrorVisibility => HasError ? Visibility.Visible : Visibility.Collapsed;
    public Visibility HomeVisibility => TabVisibility("home");
    public Visibility AccountVisibility => TabVisibility("account");
    public Visibility SettingsVisibility => TabVisibility("settings");

    public Brush HomeTabBackground => TabBackground("home");
    public Brush AccountTabBackground => TabBackground("account");
    public Brush SettingsTabBackground => TabBackground("settings");
    public Brush HomeTabForeground => TabForeground("home");
    public Brush AccountTabForeground => TabForeground("account");
    public Brush SettingsTabForeground => TabForeground("settings");

    private Task ShowTabAsync(string tab)
    {
        if (StringComparer.Ordinal.Equals(_activeTab, tab))
        {
            return Task.CompletedTask;
        }

        _activeTab = tab;
        OnPropertyChanged(nameof(HomeVisibility));
        OnPropertyChanged(nameof(AccountVisibility));
        OnPropertyChanged(nameof(SettingsVisibility));
        OnPropertyChanged(nameof(HomeTabBackground));
        OnPropertyChanged(nameof(AccountTabBackground));
        OnPropertyChanged(nameof(SettingsTabBackground));
        OnPropertyChanged(nameof(HomeTabForeground));
        OnPropertyChanged(nameof(AccountTabForeground));
        OnPropertyChanged(nameof(SettingsTabForeground));
        if (StringComparer.Ordinal.Equals(tab, "home"))
        {
            HomeTabActivated?.Invoke(this, EventArgs.Empty);
        }
        return Task.CompletedTask;
    }

    private async Task RefreshAsync()
    {
        HasError = false;
        ErrorMessage = "";
        StatusMessage = "正在检查后端连接...";

        try
        {
            var snapshot = await _statusProvider.LoadStatusAsync(_shutdown.Token);
            StatusMessage = snapshot.StatusMessage;
            LastUpdated = $"上次刷新：{DateTimeOffset.Now:yyyy-MM-dd HH:mm:ss}";
            App.WriteLog($"Status refreshed from {_statusProvider.SourceDescription}");
        }
        catch (Exception error)
        {
            HasError = true;
            ErrorMessage = error.Message;
            StatusMessage = "后端通信失败，前端仍可独立运行。";
            LastUpdated = $"上次尝试：{DateTimeOffset.Now:yyyy-MM-dd HH:mm:ss}";
            App.WriteLog($"Status refresh failed: {error}");
        }
    }

    private Visibility TabVisibility(string tab)
    {
        return StringComparer.Ordinal.Equals(_activeTab, tab) ? Visibility.Visible : Visibility.Collapsed;
    }

    private Brush TabBackground(string tab)
    {
        return StringComparer.Ordinal.Equals(_activeTab, tab) ? SelectedTabBackground : TransparentTabBackground;
    }

    private Brush TabForeground(string tab)
    {
        return StringComparer.Ordinal.Equals(_activeTab, tab) ? SelectedTabForeground : DefaultTabForeground;
    }
}
