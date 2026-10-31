using System.Diagnostics;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Sayso.UI;

/// <summary>录音状态与计时，只在控件可见时刷新；识别文本更新不重置录音时钟。音量由面板边缘的流光呈现。</summary>
internal sealed class RecordingFeedback : StackPanel
{
    private readonly TextBlock clock = Controls.Text("00:00", 11, 0.65);
    private readonly DispatcherQueueTimer timer;
    private readonly long startedAt;

    internal TimeSpan Elapsed => Stopwatch.GetElapsedTime(startedAt);
    internal bool IsUpdating => timer.IsRunning;

    internal RecordingFeedback(string title, long startedAt)
    {
        this.startedAt = startedAt;
        Orientation = Orientation.Horizontal;
        Spacing = 8;
        VerticalAlignment = VerticalAlignment.Center;
        Children.Add(new FontIcon { Glyph = "\uE720", FontSize = 12, Foreground = Controls.Brush("SaysoAccentText") });
        Children.Add(Controls.Text(title, 11, 0.65));
        Children.Add(clock);
        timer = DispatcherQueue.CreateTimer();
        timer.Interval = TimeSpan.FromMilliseconds(250);
        timer.Tick += (_, _) => Refresh();
        Loaded += (_, _) => { Refresh(); timer.Start(); };
        Unloaded += (_, _) => Stop();
    }

    internal void Stop() => timer.Stop();

    private void Refresh()
    {
        var elapsed = Elapsed;
        clock.Text = $"{(int)elapsed.TotalMinutes:00}:{elapsed.Seconds:00}";
    }
}
