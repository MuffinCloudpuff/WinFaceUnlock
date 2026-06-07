using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Windowing;
using Windows.Graphics;
using WinFaceUnlock.Control.App.Backends;
using WinFaceUnlock.Control.App.ViewModels;
using WinRT.Interop;

namespace WinFaceUnlock.Control.App;

public sealed partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        IControlStatusProvider statusProvider = new RealControlStatusProvider();
        ViewModel = new MainViewModel(statusProvider);
        ViewModel.HomeTabActivated += OnHomeTabActivated;
        Title = ViewModel.AppTitle;
        if (Content is FrameworkElement root)
        {
            root.DataContext = ViewModel;
            root.Loaded += OnRootLoaded;
        }

        ResizeForDesignReview();
    }

    public MainViewModel ViewModel { get; }

    private void ResizeForDesignReview()
    {
        var windowHandle = WindowNative.GetWindowHandle(this);
        var windowId = Win32Interop.GetWindowIdFromWindow(windowHandle);
        var appWindow = AppWindow.GetFromWindowId(windowId);
        appWindow.Resize(new SizeInt32(1620, 868));
    }

    private void OnRootLoaded(object sender, RoutedEventArgs args)
    {
        if (sender is FrameworkElement root
            && root.Resources["HomeEntranceStoryboard"] is Storyboard storyboard)
        {
            storyboard.Begin();
        }
    }

    private void OnHomeTabActivated(object? sender, EventArgs args)
    {
        if (Content is FrameworkElement root
            && root.Resources["HomeContentEntranceStoryboard"] is Storyboard storyboard)
        {
            storyboard.Begin();
        }
    }

    private void EnrollmentButton_OnPointerEntered(object sender, PointerRoutedEventArgs args)
    {
        AnimateEnrollmentButtonScale(1.02);
    }

    private void EnrollmentButton_OnPointerExited(object sender, PointerRoutedEventArgs args)
    {
        AnimateEnrollmentButtonScale(1.0);
    }

    private void EnrollmentButton_OnPointerPressed(object sender, PointerRoutedEventArgs args)
    {
        AnimateEnrollmentButtonScale(0.97);
    }

    private void EnrollmentButton_OnPointerReleased(object sender, PointerRoutedEventArgs args)
    {
        AnimateEnrollmentButtonScale(1.02);
    }

    private void AnimateEnrollmentButtonScale(double scale)
    {
        var storyboard = new Storyboard();
        storyboard.Children.Add(CreateScaleAnimation(scale, "ScaleX"));
        storyboard.Children.Add(CreateScaleAnimation(scale, "ScaleY"));
        storyboard.Begin();
    }

    private DoubleAnimation CreateScaleAnimation(double scale, string propertyName)
    {
        var animation = new DoubleAnimation
        {
            To = scale,
            Duration = new Duration(TimeSpan.FromMilliseconds(120)),
            EasingFunction = new CircleEase { EasingMode = EasingMode.EaseOut }
        };
        Storyboard.SetTarget(animation, EnrollmentButtonScale);
        Storyboard.SetTargetProperty(animation, propertyName);
        return animation;
    }
}
