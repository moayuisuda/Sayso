using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.Foundation;

namespace Sayso.UI;

/// <summary>
/// 流程图：左列的节点汇入中间节点，再分流到右列的节点，节点之间用带箭头的线相连。
/// 两列各自纵向堆叠，节点再多也只增加高度，窄窗口下不会溢出。没有节点的一列不画。
/// </summary>
internal static class FlowDiagram
{
    private const double HubWidth = 44;
    private const double Gap = 30;
    private const double ArrowLength = 6;

    public static UIElement Create(string sourcesTitle, IReadOnlyList<UIElement> sources, FrameworkElement hub, string targetsTitle, IReadOnlyList<UIElement> targets)
    {
        var sourceNodes = sources.Select(Node).ToList();
        var targetNodes = targets.Select(Node).ToList();
        var grid = new Grid { ColumnSpacing = Gap, RowSpacing = 8 };
        grid.RowDefinitions.Add(new() { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new() { Height = GridLength.Auto });
        var canvas = new Canvas { IsHitTestVisible = false };
        Grid.SetRow(canvas, 1);
        grid.Children.Add(canvas);
        if (sourceNodes.Count > 0) AddColumn(grid, sourcesTitle, sourceNodes);
        grid.ColumnDefinitions.Add(new() { Width = new(HubWidth) });
        hub.HorizontalAlignment = HorizontalAlignment.Center;
        hub.VerticalAlignment = VerticalAlignment.Center;
        Place(grid, hub, 1);
        if (targetNodes.Count > 0) AddColumn(grid, targetsTitle, targetNodes);
        Grid.SetColumnSpan(canvas, grid.ColumnDefinitions.Count);
        // 连线依赖节点布局后的位置，每次尺寸变化时重画。
        grid.SizeChanged += (_, _) => DrawLinks(canvas, sourceNodes, hub, targetNodes);
        return grid;
    }

    private static Border Node(UIElement content) => new()
    {
        Background = Controls.Brush("SaysoCard"), BorderBrush = Controls.Brush("SaysoLine"), BorderThickness = new(1),
        CornerRadius = new(8), Padding = new(10), Child = content
    };

    private static void AddColumn(Grid grid, string title, IReadOnlyList<Border> nodes)
    {
        grid.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
        var heading = Controls.Text(title, 12, 0.65);
        heading.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
        heading.Margin = new(4, 0, 0, 0);
        Place(grid, heading, 0);
        var column = Controls.Stack(nodes.ToArray<UIElement>());
        column.Spacing = 10;
        column.VerticalAlignment = VerticalAlignment.Center;
        Place(grid, column, 1);
    }

    /// <summary>放进最后一列的指定行。</summary>
    private static void Place(Grid grid, FrameworkElement element, int row)
    {
        Grid.SetRow(element, row);
        Grid.SetColumn(element, grid.ColumnDefinitions.Count - 1);
        grid.Children.Add(element);
    }

    private static void DrawLinks(Canvas canvas, IReadOnlyList<Border> sources, FrameworkElement hub, IReadOnlyList<Border> targets)
    {
        canvas.Children.Clear();
        var center = Bounds(hub, canvas);
        foreach (var bounds in sources.Select(node => Bounds(node, canvas)))
            DrawLink(canvas, new(bounds.Right, bounds.Top + bounds.Height / 2), new(center.Left, center.Top + center.Height / 2));
        foreach (var bounds in targets.Select(node => Bounds(node, canvas)))
            DrawLink(canvas, new(center.Right, center.Top + center.Height / 2), new(bounds.Left, bounds.Top + bounds.Height / 2));
    }

    private static Rect Bounds(FrameworkElement element, UIElement relativeTo) =>
        element.TransformToVisual(relativeTo).TransformBounds(new(0, 0, element.ActualWidth, element.ActualHeight));

    /// <summary>水平出入的三次曲线，终点留给箭头。</summary>
    private static void DrawLink(Canvas canvas, Point start, Point end)
    {
        var brush = Controls.Brush("SaysoAccentText");
        var tip = new Point(end.X - ArrowLength, end.Y);
        var middle = (start.X + tip.X) / 2;
        var figure = new PathFigure { StartPoint = start };
        figure.Segments.Add(new BezierSegment { Point1 = new(middle, start.Y), Point2 = new(middle, tip.Y), Point3 = tip });
        var geometry = new PathGeometry();
        geometry.Figures.Add(figure);
        canvas.Children.Add(new Microsoft.UI.Xaml.Shapes.Path { Data = geometry, Stroke = brush, StrokeThickness = 1.25 });
        canvas.Children.Add(new Polygon { Fill = brush, Points = [end, new(tip.X, end.Y - 3.5), new(tip.X, end.Y + 3.5)] });
    }
}
