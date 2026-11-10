using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace Sayso.UI;

/// <summary>候选框和设置页共用的 WinUI 控件与主题。</summary>
internal static class Controls
{
    public static SolidColorBrush Accent => (SolidColorBrush)Application.Current.Resources["SaysoAccent"];
    public static ElementTheme Theme { get; set; } = Application.Current.RequestedTheme == ApplicationTheme.Dark ? ElementTheme.Dark : ElementTheme.Light;
    public static Brush Brush(string key) => (Brush)((ResourceDictionary)Application.Current.Resources.ThemeDictionaries[Theme.ToString()])[key];
    public static T Accented<T>(T control) where T : Control
    {
        if (control is ListViewItem)
        {
            foreach (var state in new[] { "", "PointerOver", "Pressed" }) control.Resources[$"ListViewItemSelectionIndicator{state}Brush"] = Accent;
            control.Resources["ListViewItemBackgroundSelected"] = Brush("SaysoSelection");
            return control;
        }
        // 原生样式的 StaticResource 颜色已在控件库内解析，须覆盖控件级资源才能保持 Indigo。
        var keys = control switch
        {
            CheckBox => new[] { "CheckBoxCheckBackgroundFillChecked" },
            Slider => ["SliderThumbBackground", "SliderTrackValueFill"],
            ToggleSwitch => ["ToggleSwitchFillOn"],
            _ => throw new ArgumentException("Unsupported accent control.", nameof(control))
        };
        foreach (var key in keys)
            foreach (var state in new[] { "", "PointerOver", "Pressed" }) control.Resources[key + state] = Accent;
        var foregroundKey = control switch { CheckBox => "CheckBoxCheckGlyphForegroundChecked", ToggleSwitch => "ToggleSwitchKnobFillOn", _ => null };
        if (foregroundKey is not null)
            foreach (var state in new[] { "", "PointerOver", "Pressed" }) control.Resources[foregroundKey + state] = Application.Current.Resources["SaysoOnAccent"];
        return control;
    }
    public static TextBlock Text(string text, double size = 13, double opacity = 1) => new()
    {
        Text = text, FontSize = size, Opacity = opacity, TextWrapping = TextWrapping.Wrap, VerticalAlignment = VerticalAlignment.Center
    };
    public static TextBlock OptionalText(string text = "", double size = 12, double opacity = 1)
    {
        var label = Text(text, size, opacity);
        static void UpdateVisibility(TextBlock element) => element.Visibility = string.IsNullOrWhiteSpace(element.Text) ? Visibility.Collapsed : Visibility.Visible;
        label.RegisterPropertyChangedCallback(TextBlock.TextProperty, static (sender, _) => UpdateVisibility((TextBlock)sender));
        UpdateVisibility(label);
        return label;
    }
    public static StackPanel Stack(params UIElement[] children) => Stack(14, children);
    public static StackPanel Stack(double spacing, params UIElement[] children)
    {
        var panel = new StackPanel { Spacing = spacing };
        foreach (var child in children) panel.Children.Add(child);
        return panel;
    }
    public static StackPanel Horizontal(params UIElement[] children)
    {
        var panel = Stack(children);
        panel.Orientation = Orientation.Horizontal;
        panel.Spacing = 8;
        return panel;
    }
    public static WrapPanel Wrap(params UIElement[] children)
    {
        var panel = new WrapPanel();
        foreach (var child in children) panel.Children.Add(child);
        return panel;
    }
    public static Border Card(params UIElement[] children) => new()
    {
        Background = Brush("SaysoCard"), BorderBrush = Brush("SaysoLine"), BorderThickness = new(1),
        CornerRadius = new(10), Padding = new(14), Child = Stack(12, children)
    };
    public static UIElement Section(string title, params UIElement[] children)
    {
        var heading = Text(title, 12, 0.76);
        heading.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
        heading.Margin = new(4, 0, 0, 0);
        return Stack(6, heading, Card(children));
    }
    public static UIElement Row(string title, FrameworkElement control, string? caption = null)
    {
        var grid = new Grid { ColumnSpacing = 14 };
        grid.ColumnDefinitions.Add(new() { Width = new(112) });
        grid.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
        grid.RowDefinitions.Add(new() { Height = GridLength.Auto });
        Grid.SetColumn(control, 1);
        grid.Children.Add(control);
        if (!string.IsNullOrWhiteSpace(title))
        {
            grid.Children.Add(Text(title));
            AutomationProperties.SetName(control, title);
        }
        if (!string.IsNullOrWhiteSpace(caption))
        {
            // 说明独占第二行，标签与控件居中对齐，说明换行也不会改变对齐位置。
            grid.RowDefinitions.Add(new() { Height = GridLength.Auto });
            var hint = Text(caption, 12, 0.66);
            hint.Margin = new(0, 5, 0, 0);
            Grid.SetRow(hint, 1);
            Grid.SetColumn(hint, 1);
            grid.Children.Add(hint);
        }
        return grid;
    }
    public static Button Button(string label, Action action)
    {
        var button = new Button { Content = label, CornerRadius = new(6), FontSize = 12 };
        button.Click += (_, _) => action();
        return button;
    }
    /// `enabled` 为 false 的选项显示但不可选。
    public static ComboBox Choice(IEnumerable<(string Value, string Title)> options, string selected, Action<string> changed, Func<string, bool>? enabled = null)
    {
        var combo = new ComboBox { MinWidth = 170, MaxWidth = 300, HorizontalAlignment = HorizontalAlignment.Left, FontSize = 13 };
        foreach (var (value, title) in options) combo.Items.Add(new ComboBoxItem { Tag = value, Content = title, IsEnabled = enabled?.Invoke(value) ?? true });
        combo.SelectedItem = combo.Items.Cast<ComboBoxItem>().FirstOrDefault(i => (string)i.Tag == selected);
        combo.SelectionChanged += (_, _) => { if (combo.SelectedItem is ComboBoxItem item) changed((string)item.Tag); };
        return combo;
    }
    public static Button IconButton(string glyph, string label, Action action)
    {
        var button = new Button
        {
            Content = new FontIcon { Glyph = glyph, FontSize = 12 },
            Width = 24, Height = 24, MinWidth = 0, MinHeight = 0, Padding = new(0),
            Background = new SolidColorBrush(Colors.Transparent), BorderThickness = new(0),
            Foreground = Brush("SaysoSecondary"), CornerRadius = new(5)
        };
        AutomationProperties.SetName(button, label);
        ToolTipService.SetToolTip(button, label);
        button.Click += (_, _) => action();
        return button;
    }
    public static Button ChipButton(string label, Action action)
    {
        var button = Button(label, action);
        button.MinHeight = 24;
        button.Padding = new(8, 3, 8, 3);
        button.FontSize = 11;
        button.Background = Brush("SaysoCard");
        button.BorderThickness = new(0);
        return button;
    }
    public static TextBox Field(string value, Action<string> changed, string placeholder = "")
    {
        var field = new TextBox { Text = value, PlaceholderText = placeholder, FontSize = 13 };
        field.LostFocus += (_, _) => { if (field.Text != value) { value = field.Text; changed(value); } };
        return field;
    }
    public static PasswordBox Secret(string value, Action<string> changed)
    {
        var field = new PasswordBox { Password = value, FontSize = 13 };
        field.LostFocus += (_, _) => { if (field.Password != value) { value = field.Password; changed(value); } };
        return field;
    }
    public static ScrollViewer Page(params UIElement[] sections)
    {
        var panel = Stack(18, sections);
        panel.MaxWidth = 640;
        panel.HorizontalAlignment = HorizontalAlignment.Stretch;
        var host = new Grid { Padding = new(24, 18, 24, 20) };
        host.Children.Add(panel);
        return new ScrollViewer
        {
            Content = host, HorizontalContentAlignment = HorizontalAlignment.Stretch,
            HorizontalScrollMode = ScrollMode.Disabled, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto
        };
    }
}
