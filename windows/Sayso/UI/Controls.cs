using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace Sayso.UI;

/// <summary>与 SwiftUI Theme 对齐的公共控件：10px 卡片、8px 行、6px 控件与 Indigo 强调色。</summary>
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
    public static StackPanel Stack(params UIElement[] children)
    {
        var panel = new StackPanel { Spacing = 14 };
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
    public static Border Card(params UIElement[] children) => new()
    {
        Background = Brush("SaysoCard"), BorderBrush = Brush("SaysoLine"), BorderThickness = new(1),
        CornerRadius = new(10), Padding = new(16), Child = Stack(children)
    };
    public static UIElement Section(string title, params UIElement[] children)
    {
        var heading = Text(title, 12, 0.65);
        heading.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
        heading.Margin = new(4, 0, 0, 0);
        var panel = Stack(heading, Card(children));
        panel.Spacing = 8;
        return panel;
    }
    public static UIElement Row(string title, UIElement control, string? caption = null)
    {
        var grid = new Grid { ColumnSpacing = 16 };
        grid.ColumnDefinitions.Add(new() { Width = new(134) });
        grid.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
        var label = Text(title);
        label.HorizontalAlignment = HorizontalAlignment.Right;
        label.VerticalAlignment = VerticalAlignment.Top;
        label.Margin = new(0, 7, 0, 0);
        var content = Stack(control);
        content.Spacing = 6;
        if (!string.IsNullOrEmpty(caption)) content.Children.Add(Text(caption, 11, 0.62));
        Grid.SetColumn(content, 1);
        grid.Children.Add(label);
        grid.Children.Add(content);
        if (control is FrameworkElement element) AutomationProperties.SetName(element, title);
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
        var field = new TextBox { Text = value, PlaceholderText = placeholder, Width = 300, HorizontalAlignment = HorizontalAlignment.Left, FontSize = 13 };
        field.LostFocus += (_, _) => { if (field.Text != value) { value = field.Text; changed(value); } };
        return field;
    }
    public static PasswordBox Secret(string value, Action<string> changed)
    {
        var field = new PasswordBox { Password = value, Width = 300, HorizontalAlignment = HorizontalAlignment.Left, FontSize = 13 };
        field.LostFocus += (_, _) => { if (field.Password != value) { value = field.Password; changed(value); } };
        return field;
    }
    public static ScrollViewer Page(params UIElement[] sections)
    {
        var panel = Stack(sections);
        panel.Spacing = 24;
        panel.MaxWidth = 600;
        panel.HorizontalAlignment = HorizontalAlignment.Stretch;
        var host = new Grid { Padding = new(32, 28, 32, 28) };
        host.Children.Add(panel);
        return new ScrollViewer
        {
            Content = host, HorizontalContentAlignment = HorizontalAlignment.Stretch,
            HorizontalScrollMode = ScrollMode.Disabled, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto
        };
    }
}
