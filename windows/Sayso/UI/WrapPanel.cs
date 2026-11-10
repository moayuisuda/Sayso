using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Foundation;

namespace Sayso.UI;

/// <summary>按子元素的实际宽度换行，同一行内垂直居中，供候选框与扩展页签共用。</summary>
internal sealed class WrapPanel : Panel
{
    private const double Gap = 6;

    protected override Size MeasureOverride(Size availableSize)
    {
        foreach (var child in Children) child.Measure(availableSize);
        var rows = Rows(availableSize.Width);
        var width = rows.Count == 0 ? 0 : rows.Max(row => row.Width);
        return new Size(width, rows.Sum(row => row.Height) + Gap * Math.Max(rows.Count - 1, 0));
    }

    protected override Size ArrangeOverride(Size finalSize)
    {
        double y = 0;
        foreach (var row in Rows(finalSize.Width))
        {
            double x = 0;
            foreach (var child in row.Children)
            {
                var size = child.DesiredSize;
                child.Arrange(new Rect(x, y + (row.Height - size.Height) / 2, size.Width, size.Height));
                x += size.Width + Gap;
            }
            y += row.Height + Gap;
        }
        return finalSize;
    }

    /// <summary>按已测量的尺寸分行：放不下的子元素换到下一行。</summary>
    private List<(List<UIElement> Children, double Width, double Height)> Rows(double maxWidth)
    {
        var rows = new List<(List<UIElement> Children, double Width, double Height)>();
        foreach (var child in Children)
        {
            if (child.Visibility == Visibility.Collapsed) continue;
            var size = child.DesiredSize;
            if (rows.Count == 0 || rows[^1].Width + Gap + size.Width > maxWidth)
            {
                rows.Add((new List<UIElement> { child }, size.Width, size.Height));
                continue;
            }
            var row = rows[^1];
            row.Children.Add(child);
            rows[^1] = (row.Children, row.Width + Gap + size.Width, Math.Max(row.Height, size.Height));
        }
        return rows;
    }
}
