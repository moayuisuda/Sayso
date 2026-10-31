using System.Diagnostics;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Sayso.UI;

/// <summary>从真实麦克风电平采样，只在控件可见时刷新；识别文本更新不重置录音时钟。</summary>
internal sealed class RecordingFeedback : StackPanel
{
    private static readonly double[] Peaks = [6, 10, 14, 10, 6];
    private readonly Border[] bars;
    private readonly TextBlock clock = Controls.Text("00:00", 11, 0.65);
    private readonly DispatcherQueueTimer timer;
    private readonly Func<float> readLevel;
    private readonly long startedAt;

    internal TimeSpan Elapsed => Stopwatch.GetElapsedTime(startedAt);
    internal bool IsUpdating => timer.IsRunning;

    internal RecordingFeedback(string title, Func<float> readLevel, long startedAt)
    {
        this.readLevel = readLevel;
        this.startedAt = startedAt;
        Orientation = Orientation.Horizontal;
        Spacing = 8;
        VerticalAlignment = VerticalAlignment.Center;
        bars = Peaks.Select(_ => new Border
        {
            Width = 3, Height = 4, CornerRadius = new(2), Background = Controls.Brush("SaysoAccentText"),
            VerticalAlignment = VerticalAlignment.Center
        }).ToArray();
        var meter = Controls.Horizontal(bars);
        meter.Spacing = 2;
        meter.Height = 18;
        Children.Add(meter);
        Children.Add(Controls.Text(title, 11, 0.65));
        Children.Add(clock);
        timer = DispatcherQueue.CreateTimer();
        timer.Interval = TimeSpan.FromMilliseconds(60);
        timer.Tick += (_, _) => Refresh();
        Loaded += (_, _) => { Refresh(); timer.Start(); };
        Unloaded += (_, _) => Stop();
    }

    internal void Stop() => timer.Stop();

    private void Refresh()
    {
        var level = readLevel();
        for (var index = 0; index < bars.Length; index++) bars[index].Height = 4 + level * Peaks[index];
        var elapsed = Elapsed;
        clock.Text = $"{(int)elapsed.TotalMinutes:00}:{elapsed.Seconds:00}";
    }
}
