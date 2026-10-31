using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Foundation;

namespace Sayso.UI;

/// <summary>按按钮的实际宽度换行，供候选框与扩展页签共用。</summary>
internal sealed class WrapPanel : Panel
{
    private const double Gap = 6;
    protected override Size MeasureOverride(Size availableSize)
    {
        double x = 0, y = 0, height = 0, width = 0;
        foreach (var child in Children)
        {
            child.Measure(availableSize);
            var size = child.DesiredSize;
            if (x > 0 && x + size.Width > availableSize.Width) { y += height + Gap; x = 0; height = 0; }
            x += size.Width + Gap;
            width = Math.Max(width, x - Gap);
            height = Math.Max(height, size.Height);
        }
        return new Size(width, y + height);
    }
    protected override Size ArrangeOverride(Size finalSize)
    {
        double x = 0, y = 0, height = 0;
        foreach (var child in Children)
        {
            var size = child.DesiredSize;
            if (x > 0 && x + size.Width > finalSize.Width) { y += height + Gap; x = 0; height = 0; }
            child.Arrange(new Rect(x, y, size.Width, size.Height));
            x += size.Width + Gap;
            height = Math.Max(height, size.Height);
        }
        return finalSize;
    }
}
