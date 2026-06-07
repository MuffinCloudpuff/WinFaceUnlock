namespace WinFaceUnlock.Control.App.ViewModels;

public sealed class StatusItemViewModel : NotifyObject
{
    private string _title;
    private string _value;
    private string _detail;
    private bool _isHealthy;

    public StatusItemViewModel(string title, string value, string detail, bool isHealthy)
    {
        _title = title;
        _value = value;
        _detail = detail;
        _isHealthy = isHealthy;
    }

    public string Title
    {
        get => _title;
        set => SetProperty(ref _title, value);
    }

    public string Value
    {
        get => _value;
        set => SetProperty(ref _value, value);
    }

    public string Detail
    {
        get => _detail;
        set => SetProperty(ref _detail, value);
    }

    public bool IsHealthy
    {
        get => _isHealthy;
        set
        {
            if (SetProperty(ref _isHealthy, value))
            {
                OnPropertyChanged(nameof(Glyph));
            }
        }
    }

    public string Glyph => IsHealthy ? "\uE73E" : "\uE783";
}
