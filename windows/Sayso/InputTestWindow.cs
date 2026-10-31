using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Sayso.Platform;

namespace Sayso;

/// <summary>集成检查共用的真实输入窗口，确保窗口加载并获得焦点后才注入按键。</summary>
internal sealed class InputTestWindow : IDisposable
{
    internal TextBox Input { get; } = new() { Margin = new(16), VerticalAlignment = VerticalAlignment.Center, AcceptsReturn = true };
    internal Window Window { get; }
    internal nint Handle => WinRT.Interop.WindowNative.GetWindowHandle(Window);
    private readonly TaskCompletionSource loaded = new();

    internal InputTestWindow()
    {
        Input.Loaded += (_, _) => loaded.TrySetResult();
        var panel = new Grid();
        panel.Children.Add(Input);
        Window = new Window { Title = "Sayso · 输入集成检查", Content = panel };
        Window.AppWindow.Resize(new(480, 200));
    }

    internal async Task Show()
    {
        Window.Activate();
        await loaded.Task.WaitAsync(TimeSpan.FromSeconds(5));
        var deadline = DateTime.UtcNow.AddSeconds(3);
        while (NativeMethods.GetForegroundWindow() != Handle && DateTime.UtcNow < deadline) await Task.Delay(50);
        if (NativeMethods.GetForegroundWindow() != Handle || !Input.Focus(FocusState.Programmatic))
            throw new Exception("The integration input could not receive foreground focus.");
    }

    public void Dispose() => Window.Close();
}
