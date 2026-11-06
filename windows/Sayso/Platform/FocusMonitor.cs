using System.ComponentModel;
using System.Text.Json.Nodes;

namespace Sayso.Platform;

/// <summary>常驻监听前台窗口切换并上报所属应用，创建时先上报一次当前应用。须在有消息循环的线程上创建。</summary>
internal sealed class FocusMonitor : IDisposable
{
    private readonly Action<JsonNode?> report;
    private readonly NativeMethods.WinEventProc callback;
    private nint hook;

    internal FocusMonitor(Action<JsonNode?> report)
    {
        this.report = report;
        callback = OnForeground;
        hook = NativeMethods.SetWinEventHook(3, 3, 0, callback, 0, 0, 0);
        if (hook == 0) throw new Win32Exception();
        report(WindowsPlatform.AppOf(NativeMethods.GetForegroundWindow()));
    }

    private void OnForeground(nint hook, uint eventType, nint active, int objectId, int childId, uint thread, uint time) =>
        report(WindowsPlatform.AppOf(active));

    public void Dispose()
    {
        if (hook != 0) { NativeMethods.UnhookWinEvent(hook); hook = 0; }
    }
}
