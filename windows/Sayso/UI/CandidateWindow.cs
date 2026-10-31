using System.Text.Json.Nodes;
using System.Diagnostics;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Sayso.Interop;
using Sayso.Platform;
using Windows.Graphics;

namespace Sayso.UI;

/// <summary>提示按单行浮层呈现，识别与翻译内容按自然高度展开；全程不抢输入焦点。</summary>
internal sealed class CandidateWindow : Window
{
    private const double NoticeWidth = 360, ContentWidth = 420;
    private readonly CoreClient core;
    private readonly Func<string, string, string> localize;
    private readonly Action openSettings;
    private readonly RecordingGlow recordingGlow;
    private readonly CandidateDismissMonitor dismissMonitor;
    private string lastKind = "idle";
    private bool recognizingSpeech;
    private long recordingStartedAt;
    private RecordingFeedback? recordingFeedback;
    private JsonNode? renderedSession;
    private readonly NativeMethods.SubclassProc windowProcedure = BorderlessProcedure;
    private readonly Border root = new()
    {
        CornerRadius = new(8), BorderThickness = new(1),
        Background = Controls.Brush("SaysoFloatingSurface"), BorderBrush = Controls.Brush("SaysoFloatingLine"),
        VerticalAlignment = VerticalAlignment.Top
    };
    private bool visible, compact, placing, inspectionPinned;
    internal UIElement? RenderedContent => root.Child;
    internal bool RecordingGlowAnimating => recordingGlow.IsAnimating;

    public CandidateWindow(CoreClient core, Func<string, string, string> localize, Action openSettings, Func<float> readAudioLevel)
    {
        this.core = core;
        this.localize = localize;
        this.openSettings = openSettings;
        Title = "Sayso · 候选框";
        SystemBackdrop = new DesktopAcrylicBackdrop();
        // 流光叠在内容之上且独立于内容：会话内容随识别文本重建时，流光持续流动。
        recordingGlow = new RecordingGlow(readAudioLevel, root.CornerRadius.TopLeft);
        Content = new Grid { Children = { root, recordingGlow } };
        var presenter = OverlappedPresenter.Create();
        presenter.IsAlwaysOnTop = true;
        presenter.IsResizable = false;
        presenter.IsMaximizable = false;
        presenter.IsMinimizable = false;
        AppWindow.SetPresenter(presenter);
        presenter.SetBorderAndTitleBar(false, false);
        AppWindow.IsShownInSwitchers = false;
        var handle = WinRT.Interop.WindowNative.GetWindowHandle(this);
        dismissMonitor = new CandidateDismissMonitor(handle, id => Run("dismissSurface", JsonValue.Create(id)));
        if (!NativeMethods.SetWindowSubclass(handle, windowProcedure, 1, 0)) throw new InvalidOperationException("Cannot configure the candidate window frame.");
        Closed += (_, _) => { dismissMonitor.Dispose(); NativeMethods.RemoveWindowSubclass(handle, windowProcedure, 1); };
        NativeMethods.SetWindowLongPtrW(handle, -20, NativeMethods.GetWindowLongPtrW(handle, -20) | 0x08000000 | 0x80);
        NativeMethods.StyleFloatingWindow(handle);
        root.PointerEntered += (_, _) => Run("hover", JsonValue.Create(true));
        root.PointerExited += (_, _) => { if (!inspectionPinned) Run("hover", JsonValue.Create(false)); };
        root.SizeChanged += (_, _) => { if (visible) Place(); };
        AppWindow.Hide();
    }

    private async void Run(string method, JsonNode? args = null)
    {
        try { await core.Call(method, args); }
        catch (Exception error) { App.Log(error); }
    }

    internal void Dismiss()
    {
        if (renderedSession?["kind"]?.GetValue<string>() == "presenting")
            Run("dismissSurface", JsonValue.Create(renderedSession["id"]!.GetValue<string>()));
    }

    internal void Update(JsonNode session)
    {
        // 悬停仅改变核心的自动隐藏计时；相同内容必须保留控件实例，避免在点击前重建按钮。
        if (JsonNode.DeepEquals(renderedSession, session)) return;
        renderedSession = session.DeepClone();
        var kind = session["kind"]!.GetValue<string>();
        dismissMonitor.Watch(kind == "presenting" ? session["id"]!.GetValue<string>() : null);
        if (kind == "recording" && lastKind != "recording") recordingStartedAt = Stopwatch.GetTimestamp();
        recognizingSpeech = kind == "processing" && (lastKind == "recording" || recognizingSpeech);
        lastKind = kind;
        recordingFeedback?.Stop();
        recordingFeedback = null;
        if (kind == "recording") recordingGlow.Start();
        else recordingGlow.Stop();
        if (kind == "idle") { visible = false; root.Child = null; AppWindow.Hide(); return; }
        var actions = Controls.Horizontal(
            Controls.IconButton("\uE713", localize("打开设置", "Open settings"), openSettings),
            Controls.IconButton("\uE711", kind == "presenting" ? localize("关闭候选框（Enter）", "Close panel (Enter)") : localize("取消（Esc）", "Cancel (Esc)"),
                () => Run(kind == "presenting" ? "dismissSurface" : "cancel", kind == "presenting" ? JsonValue.Create(session["id"]!.GetValue<string>()) : null)));
        actions.Spacing = 2;

        var blocks = Controls.Stack();
        blocks.Spacing = 10;
        var sections = kind == "presenting" ? session["sections"]!.AsArray() : null;
        // 单个无操作的状态块不需要标题栏、底栏或独立卡片。
        compact = sections is { Count: 1 } && sections[0]!["title"] is null &&
            sections[0]!["blocks"] is JsonArray { Count: 1 } notices &&
            notices[0]!["type"]!.GetValue<string>() == "status" &&
            (notices[0]!["actions"] is not JsonArray { Count: > 0 }) && session["working"]?.GetValue<bool>() != true;

        if (sections is not null)
        {
            foreach (var section in sections)
            {
                if (section!["title"]?.GetValue<string>() is { } title) blocks.Children.Add(Controls.Text(title, 11, 0.6));
                foreach (var block in section["blocks"]!.AsArray())
                {
                    var actionBlock = block!;
                    blocks.Children.Add(BlockRenderer.Render(actionBlock, action => Run("candidateAction", new JsonObject
                    {
                        ["surface"] = session["id"]!.GetValue<string>(), ["source"] = section["sourceId"]!.GetValue<string>(),
                        ["block"] = actionBlock["id"]!.GetValue<string>(), ["action"] = action.DeepClone()
                    }), localize, BlockRenderer.Placement.Candidate));
                }
            }
            if (blocks.Children.Count == 0 && session["placeholder"]?.GetValue<string>() is { } placeholder)
                blocks.Children.Add(TranscriptText.Create(placeholder));
        }
        else
        {
            var text = session["text"]?.GetValue<string>();
            var placeholder = kind == "recording" ? localize("请开始说话…", "Start speaking…")
                : recognizingSpeech ? localize("正在识别语音…", "Recognizing speech…") : localize("正在处理…", "Working…");
            blocks.Children.Add(TranscriptText.Create(string.IsNullOrEmpty(text) ? placeholder : text));
            if (kind == "recording")
            {
                var commands = session["commands"]!.AsArray().Select(command => command!.GetValue<string>());
                var hint = string.Join(" · ", commands.Append(localize("Esc 取消", "Esc to cancel")));
                blocks.Children.Add(Controls.Text(hint, 11, 0.6));
            }
        }

        if (compact)
        {
            var notice = new Grid { Padding = new(12, 10, 8, 10), ColumnSpacing = 12 };
            notice.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
            notice.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
            notice.Children.Add(blocks);
            Grid.SetColumn(actions, 1);
            notice.Children.Add(actions);
            root.Child = notice;
        }
        else
        {
            var working = kind == "processing" || session["working"]?.GetValue<bool>() == true;
            var title = kind == "recording" ? localize("正在聆听", "Listening")
                : recognizingSpeech ? localize("正在识别", "Recognizing") : working ? localize("处理中", "Working") : "Sayso";
            UIElement identity;
            if (kind == "recording") identity = recordingFeedback = new RecordingFeedback(title, recordingStartedAt);
            else
            {
                UIElement icon = working ? new ProgressRing { Width = 12, Height = 12, IsActive = true, Foreground = Controls.Brush("SaysoAccentText") }
                    : new FontIcon { Glyph = "\uE8F2", FontSize = 12, Foreground = Controls.Brush("SaysoAccentText") };
                var status = Controls.Horizontal(icon, Controls.Text(title, 11, 0.65));
                status.Spacing = 6;
                identity = status;
            }
            var header = new Grid { Padding = new(14, 6, 8, 4) };
            header.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
            header.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
            header.Children.Add(identity);
            Grid.SetColumn(actions, 1);
            header.Children.Add(actions);
            var scroll = new ScrollViewer
            {
                Content = blocks, MaxHeight = kind == "presenting" ? 320 : 120, Padding = new(14, 2, 14, 12),
                HorizontalContentAlignment = HorizontalAlignment.Stretch, HorizontalScrollMode = ScrollMode.Disabled,
                HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled, VerticalScrollBarVisibility = ScrollBarVisibility.Auto
            };
            var panel = Controls.Stack(header, scroll);
            panel.Spacing = 0;
            root.Child = panel;
        }
        root.Width = compact ? NoticeWidth : ContentWidth;
        visible = true;
        Place();
        AppWindow.Show(false);
    }

    private void Place()
    {
        if (placing) return;
        placing = true;
        try
        {
            root.Measure(new Windows.Foundation.Size(root.Width, double.PositiveInfinity));
            var scale = NativeMethods.GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0;
            var size = new SizeInt32((int)Math.Ceiling(root.Width * scale), Math.Max(1, (int)Math.Ceiling(root.DesiredSize.Height * scale)));
            // WM_NCCALCSIZE 已让客户区铺满窗口。ResizeClient 仍会按原生样式补偿边缘，
            // 导致 Acrylic 背景比内容多出一圈；这里直接设置整个无边框窗口的尺寸。
            if (AppWindow.Size.Width != size.Width || AppWindow.Size.Height != size.Height) AppWindow.Resize(size);
            var foreground = NativeMethods.GetForegroundWindow();
            var area = DisplayArea.GetFromWindowId(Microsoft.UI.Win32Interop.GetWindowIdFromWindow(foreground), DisplayAreaFallback.Primary).WorkArea;
            AppWindow.Move(new PointInt32(area.X + (area.Width - AppWindow.Size.Width) / 2, area.Y + area.Height - AppWindow.Size.Height - (int)(40 * scale)));
        }
        finally { placing = false; }
    }

    internal void ValidateCompactNotice()
    {
        root.UpdateLayout();
        Place();
        var handle = WinRT.Interop.WindowNative.GetWindowHandle(this);
        var scale = NativeMethods.GetDpiForWindow(handle) / 96.0;
        var width = AppWindow.ClientSize.Width / scale;
        var height = AppWindow.ClientSize.Height / scale;
        if (!visible || !AppWindow.IsVisible || !compact || height is < 40 or > 72 || width is < 300 or > 400)
            throw new InvalidOperationException($"A single status must fit in a compact notice; actual size is {width:F0} × {height:F0}.");
        if (NativeMethods.GetForegroundWindow() == handle) throw new InvalidOperationException("The candidate panel stole input focus.");
        if (AppWindow.Size.Width != AppWindow.ClientSize.Width || AppWindow.Size.Height != AppWindow.ClientSize.Height)
            throw new InvalidOperationException("The candidate window retained a nonclient frame around its themed surface.");
        ValidateContentSurface();
    }

    internal RecordingFeedback ValidateRecordingFeedback(TimeSpan minimumElapsed)
    {
        ValidateContentSurface();
        if (recordingFeedback is not { IsUpdating: true } feedback || feedback.Elapsed < minimumElapsed)
            throw new InvalidOperationException("Recording feedback stopped updating or its clock was reset by a transcript update.");
        if (!recordingGlow.IsAnimating) throw new InvalidOperationException("The recording glow is not following the microphone level.");
        if (!AppWindow.IsVisible || NativeMethods.GetForegroundWindow() == WinRT.Interop.WindowNative.GetWindowHandle(this))
            throw new InvalidOperationException("Recording feedback must remain visible without taking input focus.");
        return feedback;
    }

    internal void ValidateContentSurface()
    {
        root.UpdateLayout();
        Place();
        var scale = root.XamlRoot.RasterizationScale;
        var origin = root.TransformToVisual(null).TransformPoint(new(0, 0));
        var geometry = $"client={AppWindow.ClientSize.Width}x{AppWindow.ClientSize.Height}, xaml={root.XamlRoot.Size}, root={root.ActualWidth}x{root.ActualHeight}, origin={origin}, scale={scale}";
        if (Math.Abs(root.XamlRoot.Size.Width * scale - AppWindow.ClientSize.Width) > 1 ||
            Math.Abs(root.XamlRoot.Size.Height * scale - AppWindow.ClientSize.Height) > 1)
            throw new InvalidOperationException("The XAML surface does not fill the native client: " + geometry);
        if (Math.Abs(origin.X * scale) > 1 || Math.Abs(origin.Y * scale) > 1 ||
            Math.Abs((root.XamlRoot.Size.Width - root.ActualWidth) * scale) > 1 ||
            Math.Abs((root.XamlRoot.Size.Height - root.ActualHeight) * scale) > 1)
            throw new InvalidOperationException("The candidate content leaves an exposed backdrop rim: " + geometry);
    }

    private static nint BorderlessProcedure(nint hwnd, uint message, nuint wparam, nint lparam, nuint id, nuint data)
    {
        // WinUI 会维护原生窗口样式；直接让客户区铺满窗口，避免系统对话框边缘与主题表面叠加。
        return message == 0x0083 ? 0 : NativeMethods.DefSubclassProc(hwnd, message, wparam, lparam);
    }

    internal void PrepareForInspection()
    {
        // 仅供隔离测试进程：先验证正式窗口行为，再将同一个浮层暴露给桌面窗口检查工具。
        inspectionPinned = true;
        var handle = WinRT.Interop.WindowNative.GetWindowHandle(this);
        NativeMethods.SetWindowLongPtrW(handle, -20, NativeMethods.GetWindowLongPtrW(handle, -20) & ~0x08000080);
        AppWindow.IsShownInSwitchers = true;
        NativeMethods.StyleFloatingWindow(handle);
    }
}
