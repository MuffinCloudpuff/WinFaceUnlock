namespace WinFaceUnlock.Setup.App.ViewModels;

public enum SetupStepState
{
    Pending,
    Running,
    Succeeded,
    Failed
}

public sealed class SetupStepViewModel : NotifyObject
{
    private SetupStepState _state;
    private string _title;

    public SetupStepViewModel(string key, string title)
    {
        Key = key;
        _title = title;
    }

    public string Key { get; }

    public string Title
    {
        get => _title;
        set => SetProperty(ref _title, value);
    }

    public SetupStepState State
    {
        get => _state;
        set
        {
            if (SetProperty(ref _state, value))
            {
                OnPropertyChanged(nameof(Glyph));
            }
        }
    }

    public string Glyph => State switch
    {
        SetupStepState.Running => "\uE895",
        SetupStepState.Succeeded => "\uE73E",
        SetupStepState.Failed => "\uE783",
        _ => "\uE916"
    };
}
