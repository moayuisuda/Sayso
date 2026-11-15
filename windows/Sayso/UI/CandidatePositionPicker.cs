using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Sayso.UI;

/// <summary>在同一块屏幕上点选候选框的角落。</summary>
internal sealed class CandidatePositionPicker : UserControl
{
    private readonly Dictionary<string, (Button Button, Border Preview, string Title)> options = [];
    private readonly TextBlock selectedTitle = Controls.Text("", 12, 0.76);
    private readonly string selectedStatus;

    public CandidatePositionPicker(JsonArray positions, string selectedStatus, Action<string> changed)
    {
        this.selectedStatus = selectedStatus;
        var screen = new Grid { Width = 176, Height = 100 };
        AutomationProperties.SetAutomationId(screen, "CandidatePositionScreen");

        foreach (var position in positions)
        {
            var value = position!["value"]!.GetValue<string>();
            var title = position["title"]!.GetValue<string>();
            var horizontal = position["horizontal"]!.GetValue<int>() < 0 ? HorizontalAlignment.Left : HorizontalAlignment.Right;
            var vertical = position["vertical"]!.GetValue<int>() < 0 ? VerticalAlignment.Top : VerticalAlignment.Bottom;
            var bracket = new Border
            {
                Width = 12, Height = 12, HorizontalAlignment = horizontal, VerticalAlignment = vertical,
                BorderBrush = Controls.Brush("SaysoSecondary"),
                BorderThickness = new(horizontal == HorizontalAlignment.Left ? 1 : 0, vertical == VerticalAlignment.Top ? 1 : 0,
                    horizontal == HorizontalAlignment.Right ? 1 : 0, vertical == VerticalAlignment.Bottom ? 1 : 0)
            };
            var preview = new Border
            {
                Width = 24, Height = 10, CornerRadius = new(2), Background = Controls.Brush("SaysoAccentText"),
                HorizontalAlignment = horizontal, VerticalAlignment = vertical, Margin = new(4)
            };
            AutomationProperties.SetAutomationId(preview, "CandidatePositionPreview");
            var corner = new Grid { Width = 32, Height = 22, Children = { bracket, preview } };
            var button = new Button
            {
                Content = corner, Width = 44, Height = 34, MinWidth = 0, MinHeight = 0, Margin = new(3),
                HorizontalAlignment = horizontal, VerticalAlignment = vertical,
                HorizontalContentAlignment = HorizontalAlignment.Center, VerticalContentAlignment = VerticalAlignment.Center,
                Padding = new(0), CornerRadius = new(5), BorderThickness = new(0),
                Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent)
            };
            AutomationProperties.SetName(button, title);
            AutomationProperties.SetAutomationId(button, value);
            ToolTipService.SetToolTip(button, title);
            button.Click += (_, _) => changed(value);
            screen.Children.Add(button);
            options.Add(value, (button, preview, title));
        }

        var frame = new Border
        {
            Child = screen, CornerRadius = new(8), BorderThickness = new(1),
            Background = Controls.Brush("SaysoCard"), BorderBrush = Controls.Brush("SaysoSecondary"),
            HorizontalAlignment = HorizontalAlignment.Left
        };
        Content = Controls.Stack(6, frame, selectedTitle);
        HorizontalAlignment = HorizontalAlignment.Left;
    }

    public void Select(string value)
    {
        foreach (var (key, option) in options)
        {
            var selected = key == value;
            AutomationProperties.SetItemStatus(option.Button, selected ? selectedStatus : "");
            option.Preview.Visibility = selected ? Visibility.Visible : Visibility.Collapsed;
        }
        selectedTitle.Text = options[value].Title;
    }
}
