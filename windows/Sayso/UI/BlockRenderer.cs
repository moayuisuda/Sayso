using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;

namespace Sayso.UI;

/// <summary>候选框与扩展页签共用唯一块渲染器，支持协议中的全部四种块与块操作。</summary>
internal static class BlockRenderer
{
    internal enum Placement { Tab, Candidate }

    public static UIElement Render(JsonNode block, Action<JsonNode> perform, Func<string, string, string> localize, Placement placement = Placement.Tab)
    {
        var content = Controls.Stack();
        content.Spacing = 6;
        var inlineActions = new HashSet<string>();
        Grid? termRow = null;
        string? Field(string key) => block[key]?.GetValue<string>();
        switch (Field("type"))
        {
            case "text":
                var text = TranscriptText.Create();
                var paragraph = new Paragraph();
                var highlighter = new TextHighlighter { Background = Controls.Brush("SaysoSelection"), Foreground = Controls.Brush("SaysoAccentText") };
                var offset = 0;
                foreach (var item in block["runs"]!.AsArray())
                {
                    var run = new Run { Text = item!["text"]!.GetValue<string>() };
                    if (item["style"]?.GetValue<string>() == "highlight")
                    {
                        highlighter.Ranges.Add(new TextRange { StartIndex = offset, Length = run.Text.Length });
                    }
                    if (item["style"]?.GetValue<string>() == "muted") run.Foreground = Controls.Brush("SaysoSecondary");
                    if (item["action"]?.GetValue<string>() is { } actionId)
                    {
                        inlineActions.Add(actionId);
                        var link = new Hyperlink { FontWeight = Microsoft.UI.Text.FontWeights.Normal }; link.Inlines.Add(run);
                        link.Click += (_, _) =>
                        {
                            var action = block["actions"]?.AsArray().FirstOrDefault(a => a!["id"]?.GetValue<string>() == actionId);
                            if (action is not null) perform(action);
                        };
                        paragraph.Inlines.Add(link);
                    }
                    else paragraph.Inlines.Add(run);
                    offset += run.Text.Length;
                }
                text.Blocks.Add(paragraph);
                text.TextHighlighters.Add(highlighter);
                content.Children.Add(text);
                break;
            case "term":
                var term = SingleLine(Field("term")!, 13);
                term.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
                term.Foreground = Controls.Brush("SaysoAccentText");
                var speak = Controls.IconButton("\uE767", localize("朗读", "Speak"), () => perform(new JsonObject { ["type"] = "speak", ["text"] = Field("term"), ["language"] = Field("language") }));
                // 词对与操作共享一行；为两侧文字分别保留空间，长内容省略并通过提示显示全文。
                termRow = new Grid { ColumnSpacing = 6, MinHeight = 28 };
                termRow.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
                termRow.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
                var pair = new Grid { ColumnSpacing = 10 };
                pair.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
                pair.Children.Add(term);
                if (Field("gloss") is { Length: > 0 } gloss)
                {
                    pair.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
                    var meaning = SingleLine(gloss, 12, 0.8);
                    Grid.SetColumn(meaning, 1);
                    pair.Children.Add(meaning);
                }
                if (Field("detail") is { Length: > 0 } detail)
                {
                    pair.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
                    var note = SingleLine(detail, 11, 0.65);
                    Grid.SetColumn(note, pair.ColumnDefinitions.Count - 1);
                    pair.Children.Add(note);
                }
                Grid.SetColumn(speak, 1);
                termRow.Children.Add(pair);
                termRow.Children.Add(speak);
                content.Children.Add(termRow);
                break;
            case "card":
                var title = Controls.Text(Field("title")!, 14);
                title.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
                content.Children.Add(title);
                if (Field("subtitle") is { } subtitle) content.Children.Add(Controls.Text(subtitle, 12, 0.65));
                if (Field("body") is { } body) content.Children.Add(Controls.Text(body));
                break;
            case "status":
                var status = new Grid { ColumnSpacing = 8 };
                status.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
                status.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
                var (glyph, color) = Field("tone") switch
                {
                    "error" => ("\uEA39", "SaysoCritical"),
                    "warning" => ("\uE7BA", "SaysoWarning"),
                    "success" => ("\uE73E", "SaysoSuccess"),
                    _ => ("\uE946", "SaysoSecondary")
                };
                glyph = Field("icon") switch
                {
                    "exclamationmark.triangle" or "exclamationmark.triangle.fill" => "\uE7BA",
                    "checkmark.circle" or "checkmark.circle.fill" => "\uE73E",
                    "info.circle" or "info.circle.fill" => "\uE946",
                    "exclamationmark.circle" or "exclamationmark.circle.fill" => "\uEA39",
                    _ => glyph
                };
                status.Children.Add(new FontIcon { Glyph = glyph, FontSize = 15, Foreground = Controls.Brush(color), VerticalAlignment = VerticalAlignment.Center });
                var message = Controls.Text(Field("message")!, 13);
                message.LineHeight = 20;
                Grid.SetColumn(message, 1);
                status.Children.Add(message);
                content.Children.Add(status);
                break;
            default: throw new InvalidOperationException($"Unsupported block type: {Field("type")}");
        }
        if (block["actions"] is JsonArray { Count: > 0 } actions)
        {
            Panel panel = termRow is null ? new WrapPanel() : new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center };
            foreach (var action in actions)
            {
                // 高亮片段已提供操作入口，与 macOS 一致，不再重复生成按钮。
                if (action!["id"]?.GetValue<string>() is { } id && inlineActions.Contains(id)) continue;
                if (placement == Placement.Candidate && action["type"]!.GetValue<string>() == "delete") continue;
                var label = action!["label"]?.GetValue<string>() ?? action["type"]!.GetValue<string>() switch
                { "speak" => localize("朗读", "Speak"), "copy" => localize("复制", "Copy"), "open" => localize("打开", "Open"), "insert" => localize("输入", "Insert"), "delete" => localize("删除", "Delete"), _ => localize("执行", "Run") };
                var button = Controls.ChipButton(label, () => perform(action));
                panel.Children.Add(button);
            }
            if (panel.Children.Count > 0)
            {
                if (termRow is null) content.Children.Add(panel);
                else
                {
                    termRow.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
                    Grid.SetColumn(panel, 2);
                    termRow.Children.Add(panel);
                }
            }
        }
        // 浮层已经有统一表面；页签条目保留独立卡片，避免候选框出现卡片套卡片。
        return placement == Placement.Candidate ? content : new Border { Padding = new(12), CornerRadius = new(8), Background = Controls.Brush("SaysoCard"), Child = content };
    }

    private static TextBlock SingleLine(string text, double size, double opacity = 1)
    {
        var label = Controls.Text(text, size, opacity);
        label.TextWrapping = TextWrapping.NoWrap;
        label.TextTrimming = TextTrimming.CharacterEllipsis;
        label.MaxLines = 1;
        ToolTipService.SetToolTip(label, text);
        return label;
    }
}
