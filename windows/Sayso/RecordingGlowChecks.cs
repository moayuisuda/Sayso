using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Sayso.UI;

namespace Sayso;

/// <summary>在真实合成器中检查音量边界与深浅主题，使用正式流光组件，不替换绘制实现。</summary>
internal static class RecordingGlowChecks
{
    private static Window? inspection;

    internal static void Show()
    {
        var panels = new StackPanel { Spacing = 20, Margin = new(20) };
        foreach (var level in new[] { 0f, 0.5f, 1f })
        {
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 20 };
            foreach (var theme in new[] { ElementTheme.Dark, ElementTheme.Light })
            {
                var resources = (ResourceDictionary)Application.Current.Resources.ThemeDictionaries[theme.ToString()];
                var panel = new Grid { Width = 420, Height = 150, RequestedTheme = theme };
                panel.Children.Add(new Border
                {
                    CornerRadius = new(8), Background = (Brush)resources["SaysoFloatingSurface"],
                    BorderBrush = (Brush)resources["SaysoFloatingLine"], BorderThickness = new(1), Padding = new(14),
                    Child = new StackPanel { Spacing = 12, Children =
                    {
                        new TextBlock { Text = $"{theme} · {level:P0}", FontSize = 13 },
                        new TextBlock { Text = "录音内容应始终清晰，不被光晕遮挡。\nThe center must stay readable.", FontSize = 15, TextWrapping = TextWrapping.Wrap },
                        new TextBlock { Text = "边缘流动 · 文字区域透明 · 圆角裁剪", FontSize = 12 }
                    }}
                });
                var glow = new RecordingGlow(() => level, 8);
                glow.Loaded += (_, _) => glow.Start();
                panel.Children.Add(glow);
                row.Children.Add(panel);
            }
            panels.Children.Add(row);
        }
        inspection = new Window { Title = "Sayso · 流光验证", Content = panels };
        inspection.AppWindow.Resize(new(1340, 850));
        inspection.Activate();
    }
}
