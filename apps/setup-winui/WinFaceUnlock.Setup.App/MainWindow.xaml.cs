using System;
using System.ComponentModel;
using Microsoft.UI.Xaml;
using WinFaceUnlock.Setup.App.ViewModels;

namespace WinFaceUnlock.Setup.App;

public sealed partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();
        ViewModel = new MainViewModel(this, Close);
        ViewModel.PropertyChanged += OnViewModelPropertyChanged;
        Title = ViewModel.AppTitle;
        if (Content is FrameworkElement root)
        {
            root.DataContext = ViewModel;
        }
    }

    public MainViewModel ViewModel { get; }

    private void OnViewModelPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(MainViewModel.AppTitle))
        {
            Title = ViewModel.AppTitle;
        }
    }
}
