using System.Diagnostics;
using System.Numerics;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace Sayso.UI;

/// <summary>
/// 录音时面板边缘的流光，与 macOS 的 FlowingBorder 一致：几层柔光沿边缘以不同速度、方向流动，
/// 色标位置与亮度各自起伏，说话越响光晕越宽越亮。
/// 每帧（CompositionTarget.Rendering）读取最新音量并平滑，直接修改合成层属性，不经过 XAML 布局；
/// 会话内容随识别文本重建时它保持不变，流动不会中断。
/// </summary>
internal sealed class RecordingGlow : Grid
{
    // 音量上升快、回落稍慢：开口说话立即亮起，随音节起伏而不闪烁。
    private const double Attack = 0.05, Release = 0.2;
    // 流动速度（进度/秒）：安静时缓慢流淌，说话越响流得越快。
    private const double IdleSpeed = 0.1, SpeakingSpeed = 0.5;
    private const double FadeSeconds = 0.25;

    private readonly Func<float> readLevel;
    private readonly float cornerRadius;
    private readonly ContainerVisual container;
    private readonly CompositionRoundedRectangleGeometry clip;
    private readonly EdgeLine edge;
    private readonly GlowLayer[] glows;
    private double level, flow, presence;
    private bool active;
    private long lastTick;

    /// <summary>正在逐帧刷新；停止录音并淡出后结束。</summary>
    internal bool IsAnimating { get; private set; }

    internal RecordingGlow(Func<float> readLevel, double cornerRadius)
    {
        this.readLevel = readLevel;
        this.cornerRadius = (float)cornerRadius;
        IsHitTestVisible = false;
        var compositor = ElementCompositionPreview.GetElementVisual(this).Compositor;
        var palette = ((LinearGradientBrush)Application.Current.Resources["SaysoRecordingGlow"]).GradientStops.Select(stop => stop.Color).ToArray();

        // 光只向面板内侧晕开：整体裁剪到面板的圆角矩形内。
        clip = compositor.CreateRoundedRectangleGeometry();
        clip.CornerRadius = new(this.cornerRadius);
        container = compositor.CreateContainerVisual();
        container.Clip = compositor.CreateGeometricClip(clip);
        container.Opacity = 0;
        // 由外到内：外层大范围柔光、中层光晕、清晰的边线。反向的一层让光团交汇而不是整体旋转。
        glows =
        [
            new(compositor, palette, blur: (12, 48), opacity: (0, 0.95), speed: 0.45, seed: 4.3),
            new(compositor, palette, blur: (6, 20), opacity: (0.3, 1), speed: -0.7, seed: 2.1),
        ];
        edge = new(compositor, palette);
        foreach (var glow in glows) container.Children.InsertAtTop(glow.Visual);
        container.Children.InsertAtTop(edge.Visual);
        ElementCompositionPreview.SetElementChildVisual(this, container);

        SizeChanged += (_, e) => Resize(new((float)e.NewSize.Width, (float)e.NewSize.Height));
        Unloaded += (_, _) => StopRendering();
    }

    /// <summary>进入录音：淡入并开始逐帧刷新；淡出途中再次开始则从当前状态继续。</summary>
    internal void Start()
    {
        active = true;
        if (IsAnimating) return;
        level = 0;
        lastTick = Stopwatch.GetTimestamp();
        Microsoft.UI.Xaml.Media.CompositionTarget.Rendering += Render;
        IsAnimating = true;
    }

    /// <summary>离开录音：淡出后停止刷新。</summary>
    internal void Stop() => active = false;

    private void StopRendering()
    {
        if (!IsAnimating) return;
        Microsoft.UI.Xaml.Media.CompositionTarget.Rendering -= Render;
        IsAnimating = false;
        presence = 0;
        container.Opacity = 0;
    }

    private void Resize(Vector2 size)
    {
        clip.Size = size;
        edge.Resize(size, cornerRadius);
        foreach (var glow in glows) glow.Resize(size, cornerRadius);
    }

    private void Render(object? sender, object e)
    {
        // 按真实帧间隔积分，与刷新率无关；长时间未绘制时不累积跳变。
        var now = Stopwatch.GetTimestamp();
        var dt = Math.Min(Stopwatch.GetElapsedTime(lastTick, now).TotalSeconds, 0.1);
        lastTick = now;
        var target = active ? readLevel() : 0;
        level += (target - level) * (1 - Math.Exp(-dt / (target > level ? Attack : Release)));
        flow += (IdleSpeed + (SpeakingSpeed - IdleSpeed) * level) * dt;
        presence = Math.Clamp(presence + (active ? dt : -dt) / FadeSeconds, 0, 1);

        container.Opacity = (float)presence;
        edge.Update(level, flow);
        foreach (var glow in glows) glow.Update(level, flow);
        if (!active && presence == 0) StopRendering();
    }

    private static float Lerp((double From, double To) range, double fraction) => (float)(range.From + (range.To - range.From) * fraction);

    /// <summary>
    /// 沿边缘游走的色彩：线性渐变绕中心旋转，色标在各自等分位置附近摆动（幅度小于间距，顺序不变），亮度各自起伏。
    /// </summary>
    private sealed class FlowGradient
    {
        private readonly CompositionLinearGradientBrush brush;
        private readonly CompositionColorGradientStop[] stops;
        private readonly Color[] palette;
        private readonly double speed, seed;

        internal CompositionBrush Brush => brush;

        internal FlowGradient(Compositor compositor, Color[] palette, double speed, double seed)
        {
            this.palette = palette;
            this.speed = speed;
            this.seed = seed;
            brush = compositor.CreateLinearGradientBrush();
            brush.MappingMode = CompositionMappingMode.Relative;
            stops = palette.Select((color, index) => compositor.CreateColorGradientStop(index / (float)(palette.Length - 1), color)).ToArray();
            foreach (var stop in stops) brush.ColorStops.Add(stop);
        }

        internal void Update(double flow)
        {
            var angle = (flow * speed + seed / 6) * 2 * Math.PI;
            var axis = new Vector2((float)Math.Cos(angle), (float)Math.Sin(angle)) * 0.5f;
            brush.StartPoint = new Vector2(0.5f) - axis;
            brush.EndPoint = new Vector2(0.5f) + axis;
            var last = stops.Length - 1;
            for (var index = 0; index < stops.Length; index++)
            {
                var brightness = 0.45 + 0.55 * (0.5 + 0.5 * Math.Sin(flow * (7 + 1.1 * index) + seed * 1.9 + index * 2.7));
                var drift = index is 0 || index == last ? 0 : 0.35 * Math.Sin(flow * (5 + 0.9 * index) + seed + index * 1.3);
                var color = palette[index];
                stops[index].Color = Color.FromArgb((byte)(brightness * 255), color.R, color.G, color.B);
                stops[index].Offset = (float)((index + drift) / last);
            }
        }
    }

    /// <summary>清晰的边线：描在面板边缘内侧，安静时也能看到光在流动。</summary>
    private sealed class EdgeLine
    {
        private static readonly (double, double) Thickness = (1.5, 3), Opacity = (0.55, 1);
        private readonly CompositionSpriteShape line;
        private readonly CompositionRoundedRectangleGeometry geometry;
        private readonly FlowGradient gradient;
        private Vector2 size;
        private float cornerRadius;

        internal ShapeVisual Visual { get; }

        internal EdgeLine(Compositor compositor, Color[] palette)
        {
            gradient = new(compositor, palette, speed: 1, seed: 0);
            geometry = compositor.CreateRoundedRectangleGeometry();
            line = compositor.CreateSpriteShape(geometry);
            line.StrokeBrush = gradient.Brush;
            Visual = compositor.CreateShapeVisual();
            Visual.Shapes.Add(line);
        }

        internal void Resize(Vector2 size, float cornerRadius)
        {
            this.size = size;
            this.cornerRadius = cornerRadius;
            Visual.Size = size;
        }

        internal void Update(double level, double flow)
        {
            gradient.Update(flow);
            // 线宽随音量变化，几何内缩半个线宽，描边始终贴着面板边缘。
            var thickness = Lerp(Thickness, level);
            line.StrokeThickness = thickness;
            geometry.Offset = new(thickness / 2);
            geometry.Size = Vector2.Max(size - new Vector2(thickness), Vector2.Zero);
            geometry.CornerRadius = new(Math.Max(cornerRadius - thickness / 2, 0));
            Visual.Opacity = Lerp(Opacity, level);
        }
    }

    /// <summary>
    /// 一层柔光：在面板边缘外侧描一圈光环，光环本身被面板裁掉，只留下它的模糊投影向内晕开。
    /// 光环的渐变透明度决定投影沿边缘的明暗分布，投影颜色在色板中缓慢流转。
    /// </summary>
    private sealed class GlowLayer
    {
        /// <summary>光环宽度：足够宽，使面板边缘处的投影接近饱和。</summary>
        private const float RingWidth = 24;
        /// <summary>图层向外留出的余量，容纳面板外侧的光环与模糊。</summary>
        private const float Margin = RingWidth + 48;
        private readonly CompositionRoundedRectangleGeometry geometry;
        private readonly ShapeVisual ring;
        private readonly DropShadow shadow;
        private readonly FlowGradient gradient;
        private readonly Color[] palette;
        private readonly (double, double) blur, opacity;
        private readonly double seed;

        internal LayerVisual Visual { get; }

        internal GlowLayer(Compositor compositor, Color[] palette, (double, double) blur, (double, double) opacity, double speed, double seed)
        {
            this.palette = palette;
            this.blur = blur;
            this.opacity = opacity;
            this.seed = seed;
            gradient = new(compositor, palette, speed, seed);
            geometry = compositor.CreateRoundedRectangleGeometry();
            var shape = compositor.CreateSpriteShape(geometry);
            shape.StrokeBrush = gradient.Brush;
            shape.StrokeThickness = RingWidth;
            ring = compositor.CreateShapeVisual();
            ring.Shapes.Add(shape);
            // LayerVisual 的投影由图层内容（光环）的透明度生成。
            shadow = compositor.CreateDropShadow();
            Visual = compositor.CreateLayerVisual();
            Visual.Shadow = shadow;
            Visual.Children.InsertAtTop(ring);
        }

        internal void Resize(Vector2 size, float cornerRadius)
        {
            Visual.Offset = new(-Margin, -Margin, 0);
            Visual.Size = ring.Size = size + new Vector2(Margin * 2);
            // 描边以几何轮廓为中心：轮廓外扩半个环宽，光环内沿正好落在面板边缘。
            geometry.Offset = new(Margin - RingWidth / 2);
            geometry.Size = size + new Vector2(RingWidth);
            geometry.CornerRadius = new(cornerRadius + RingWidth / 2);
        }

        internal void Update(double level, double flow)
        {
            gradient.Update(flow);
            shadow.BlurRadius = Lerp(blur, level);
            Visual.Opacity = Lerp(opacity, level);
            // 投影颜色在色板相邻两色之间插值流转。
            var position = (flow * 0.6 + seed) % palette.Length;
            var from = palette[(int)position];
            var to = palette[((int)position + 1) % palette.Length];
            var t = position - Math.Floor(position);
            byte Mix(byte a, byte b) => (byte)(a + (b - a) * t);
            shadow.Color = Color.FromArgb(255, Mix(from.R, to.R), Mix(from.G, to.G), Mix(from.B, to.B));
        }
    }
}
