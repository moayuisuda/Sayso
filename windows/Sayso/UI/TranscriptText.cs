using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;

namespace Sayso.UI;

/// <summary>实时识别、处理中原文和结果共用排版，避免控件默认值及高亮字重改变文字尺寸。</summary>
internal static class TranscriptText
{
    public static RichTextBlock Create(string? value = null)
    {
        var text = new RichTextBlock
        {
            FontSize = 15, FontWeight = FontWeights.Normal, LineHeight = 24,
            LineStackingStrategy = LineStackingStrategy.BlockLineHeight,
            TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true,
            Padding = new(0), OpticalMarginAlignment = OpticalMarginAlignment.None,
            VerticalAlignment = VerticalAlignment.Top
        };
        if (value is not null) text.Blocks.Add(new Paragraph { Inlines = { new Run { Text = value } } });
        return text;
    }
}
