using System.Text.Json.Nodes;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Sayso.Interop;
using Sayso.Platform;
using Windows.Graphics;

namespace Sayso.UI;

internal sealed partial class SettingsWindow : Window
{
    private readonly CoreClient? core;
    private readonly WindowsPlatform? platform;
    private readonly Grid root = new();
    private readonly StackPanel tabs = new() { Orientation = Orientation.Horizontal, Spacing = 4, HorizontalAlignment = HorizontalAlignment.Center };
    private readonly ContentControl page = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch };
    private readonly TextBlock title = Controls.Text("通用", 13);
    private readonly TextBlock status = Controls.Text("", 11, 0.6);
    private JsonArray extensions = [];
    private JsonArray languages = [];
    private JsonArray providers = [];
    internal JsonNode Settings { get; private set; } = null!;
    internal HotkeyMonitor? Hotkeys { get; set; }
    private string selected = "general";
    private bool dialogOpen;
    private int navigationVersion;

    public SettingsWindow(CoreClient? core, WindowsPlatform? platform)
    {
        this.core = core;
        this.platform = platform;
        Title = "言出 Sayso";
        SystemBackdrop = new MicaBackdrop();
        ExtendsContentIntoTitleBar = true;
        var scale = NativeMethods.GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0;
        AppWindow.Resize(new SizeInt32((int)(820 * scale), (int)(680 * scale)));
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "Sayso.ico"));
        if (AppWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.PreferredMinimumWidth = (int)(720 * scale);
            presenter.PreferredMinimumHeight = (int)(520 * scale);
        }
        root.RowDefinitions.Add(new() { Height = new(34) });
        root.RowDefinitions.Add(new() { Height = new(64) });
        root.RowDefinitions.Add(new() { Height = new(1) });
        root.RowDefinitions.Add(new() { Height = new(1, GridUnitType.Star) });
        root.RowDefinitions.Add(new() { Height = new(30) });
        var drag = new Grid();
        title.HorizontalAlignment = HorizontalAlignment.Center;
        title.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
        drag.Children.Add(title);
        root.Children.Add(drag);
        SetTitleBar(drag);
        Grid.SetRow(tabs, 1); root.Children.Add(tabs);
        var line = new Border { Background = Controls.Brush("SaysoLine") };
        Grid.SetRow(line, 2); root.Children.Add(line);
        Grid.SetRow(page, 3); root.Children.Add(page);
        status.Margin = new(20, 0, 20, 0);
        Grid.SetRow(status, 4); root.Children.Add(status);
        Content = root;
        AppWindow.Closing += (_, e) => { e.Cancel = true; AppWindow.Hide(); };
        root.ActualThemeChanged += (_, _) =>
        {
            Controls.Theme = root.ActualTheme;
            if (Settings is null) return;
            line.Background = Controls.Brush("SaysoLine");
            BuildTabs();
            Run(() => Navigate(selected));
        };
    }

    internal string L(string chinese, string english) => Settings?["nativeLanguage"]?.GetValue<string>() == "zh" ? chinese : english;
    public void Show() { AppWindow.Show(); Activate(); }
    internal void Run(Func<Task> action) => _ = Execute(action);
    private async Task Execute(Func<Task> action)
    {
        try { await action(); }
        catch (Exception error) { App.Log(error); await ShowError(error); }
    }
    internal async Task ShowError(Exception error)
    {
        if (dialogOpen) { status.Text = error.Message; return; }
        dialogOpen = true;
        try { await new ContentDialog { XamlRoot = root.XamlRoot, Title = "Sayso", Content = error.Message, CloseButtonText = "OK" }.ShowAsync(); }
        finally { dialogOpen = false; }
    }

    internal async Task<bool> Confirm(string title, string message)
    {
        if (dialogOpen) return false;
        dialogOpen = true;
        try
        {
            return await new ContentDialog { XamlRoot = root.XamlRoot, Title = title, Content = message, PrimaryButtonText = L("确认", "Confirm"), CloseButtonText = L("取消", "Cancel"), DefaultButton = ContentDialogButton.Close }.ShowAsync() == ContentDialogResult.Primary;
        }
        finally { dialogOpen = false; }
    }

    internal async Task Initialize()
    {
        Settings = (await core!.Call("settings"))!;
        languages = (await core.Call("languages"))!.AsArray();
        providers = (await core.Call("providers"))!.AsArray();
        extensions = (await core.Call("extensions"))!.AsArray();
        Hotkeys!.Settings = Settings;
        await UpdateActiveRoutes();
        BuildTabs();
        await Navigate("general");
        UpdateStatus();
    }

    internal async Task ReloadSettings()
    {
        var previousLanguage = Settings["nativeLanguage"]!.GetValue<string>();
        Settings = (await core!.Call("settings"))!;
        Hotkeys!.Settings = Settings;
        await UpdateActiveRoutes();
        UpdateStatus();
        if (previousLanguage != Settings["nativeLanguage"]!.GetValue<string>())
        {
            extensions = (await core.Call("extensions"))!.AsArray();
            BuildTabs();
            await Navigate(selected);
        }
    }

    internal async Task ReloadExtensions()
    {
        extensions = (await core!.Call("extensions"))!.AsArray();
        await UpdateActiveRoutes();
        BuildTabs();
        if (selected == "extensions" || selected.StartsWith("sayso.") || extensions.Any(e => e!["manifest"]!["id"]!.GetValue<string>() == selected)) await Navigate(selected);
    }

    internal void RefreshStore(string id) { if (selected == id) Run(() => Navigate(id)); }
    private async Task UpdateActiveRoutes() => Hotkeys!.ActiveRouteKeys = (await core!.Call("activeRouteKeys"))!.AsArray().Select(key => key!.GetValue<uint>()).ToHashSet();
    private void UpdateStatus() => status.Text = L("按住", "Hold") + " " + HotkeyMonitor.KeyName(Settings["triggerKey"]!["keyCode"]!.GetValue<uint>(), Settings["triggerKey"]!["modifiers"]!.GetValue<ulong>()) + " · " + L("说话即可输入，Esc 取消 · 关闭窗口后驻留系统托盘", "Speak to type · Esc to cancel · Closing keeps Sayso in the tray");

    internal async Task Save(string field, JsonNode? value)
    {
        Settings = await core!.MutateSettings(updated =>
        {
            updated[field] = value?.DeepClone();
            if (updated["targetLanguage"]!.GetValue<string>() == updated["nativeLanguage"]!.GetValue<string>()) throw new InvalidOperationException(L("目标语言与本地语言不能相同。", "Target and native languages must differ."));
        });
        Hotkeys!.Settings = Settings;
        UpdateStatus();
    }

    private void BuildTabs()
    {
        tabs.Children.Clear();
        var items = new List<(string Id, string Title, string Glyph)>
        {
            ("general", L("通用", "General"), "\uE713"), ("models", L("模型配置", "Models"), "\uE8F2"),
            ("extensions", L("扩展", "Extensions"), "\uEA86")
        };
        items.AddRange(extensions.Where(e => e!["enabled"]!.GetValue<bool>() && e["manifest"]!["tab"] is not null)
            .Select(e => (e!["manifest"]!["id"]!.GetValue<string>(), e["manifest"]!["tab"]!["title"]!.GetValue<string>(), "\uE8F1")));
        if (!items.Any(i => i.Id == selected)) selected = "general";
        foreach (var item in items)
        {
            var icon = new FontIcon { Glyph = item.Glyph, FontSize = 17 };
            var text = Controls.Text(item.Title, 11);
            text.HorizontalAlignment = HorizontalAlignment.Center;
            var content = Controls.Stack(icon, text); content.Spacing = 4;
            var button = new Button { Content = content, MinWidth = 76, Height = 50, Padding = new(8, 5, 8, 5), CornerRadius = new(8), BorderThickness = new(0), Tag = item.Id };
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(button, item.Title);
            button.Click += (_, _) => Run(() => Navigate(item.Id));
            tabs.Children.Add(button);
        }
        SelectTab();
    }

    private void SelectTab()
    {
        foreach (var button in tabs.Children.Cast<Button>())
        {
            var active = (string)button.Tag == selected;
            button.Background = active ? Controls.Brush("SaysoSelection") : new SolidColorBrush(Colors.Transparent);
            button.Foreground = active ? Controls.Brush("SaysoAccentText") : Controls.Brush("SaysoSecondary");
            if (active) title.Text = ((StackPanel)button.Content).Children.OfType<TextBlock>().Single().Text;
        }
    }

    internal async Task Navigate(string id)
    {
        var version = ++navigationVersion;
        selected = id;
        SelectTab();
        var content = id switch
        {
            "general" => GeneralPage(),
            "models" => await ModelsPage(),
            "extensions" => await ExtensionsPage(),
            _ => await ExtensionPage(id, "")
        };
        // 页面读取密钥等数据时允许切换页签，旧请求完成后不得覆盖当前页面。
        if (version == navigationVersion) page.Content = content;
    }

    internal void ValidateLayout()
    {
        root.UpdateLayout();
        Validate(page);
        void Validate(DependencyObject parent)
        {
            for (var i = 0; i < VisualTreeHelper.GetChildrenCount(parent); i++)
            {
                var child = VisualTreeHelper.GetChild(parent, i);
                if (child is FrameworkElement element)
                {
                    if (element.Visibility != Visibility.Visible || element.ActualWidth == 0) continue;
                    if (element is TextBlock or TextBox or PasswordBox or ComboBox)
                    {
                        var bounds = element.TransformToVisual(root).TransformBounds(new(0, 0, element.ActualWidth, element.ActualHeight));
                        if (bounds.Left < -1 || bounds.Right > root.ActualWidth + 1)
                            throw new InvalidOperationException($"Horizontal overflow in {selected}: {element.GetType().Name} ({bounds.Left:F0}–{bounds.Right:F0}, viewport {root.ActualWidth:F0}).");
                    }
                }
                Validate(child);
            }
        }
    }
}
