using System.ComponentModel;
using System.Runtime.InteropServices;

namespace Sayso.Platform;

/// <summary>非激活面板通过框外点击与前台窗口切换判断失焦，监听器只在展示结果时启用。</summary>
internal sealed class CandidateDismissMonitor : IDisposable
{
    private readonly nint window;
    private readonly Action<string> dismiss;
    private readonly NativeMethods.HookProc mouseCallback;
    private readonly NativeMethods.WinEventProc foregroundCallback;
    private nint mouseHook, foregroundHook, foreground;
    private string? surfaceId;
    private uint shownAt;

    internal CandidateDismissMonitor(nint window, Action<string> dismiss)
    {
        this.window = window;
        this.dismiss = dismiss;
        mouseCallback = OnMouse;
        foregroundCallback = OnForeground;
    }

    internal void Watch(string? id)
    {
        if (id == surfaceId) return;
        surfaceId = id;
        shownAt = unchecked((uint)Environment.TickCount);
        foreground = NativeMethods.GetForegroundWindow();
        if (id is null) { Stop(); return; }
        if (mouseHook != 0) return;
        mouseHook = NativeMethods.SetWindowsHookExW(14, mouseCallback, NativeMethods.GetModuleHandle(null), 0);
        if (mouseHook == 0) throw new Win32Exception();
        foregroundHook = NativeMethods.SetWinEventHook(3, 3, 0, foregroundCallback, 0, 0, 0);
        if (foregroundHook == 0)
        {
            var error = new Win32Exception();
            Stop();
            throw error;
        }
    }

    private nint OnMouse(int code, nint message, nint data)
    {
        if (code >= 0 && message is 0x0201 or 0x0204 or 0x0207 or 0x020B)
        {
            var mouse = Marshal.PtrToStructure<NativeMethods.MouseEvent>(data);
            if (IsCurrent(mouse.Time) && NativeMethods.GetWindowRect(window, out var bounds) &&
                (mouse.Position.X < bounds.Left || mouse.Position.X >= bounds.Right ||
                 mouse.Position.Y < bounds.Top || mouse.Position.Y >= bounds.Bottom)) dismiss(surfaceId!);
        }
        // 关闭面板不吞掉点击，目标输入框仍应收到用户的操作。
        return NativeMethods.CallNextHookEx(mouseHook, code, message, data);
    }

    private void OnForeground(nint hook, uint eventType, nint active, int objectId, int childId, uint thread, uint time)
    {
        if (IsCurrent(time) && active != 0 && active != window && active != foreground && active == NativeMethods.GetForegroundWindow())
            dismiss(surfaceId!);
    }

    private bool IsCurrent(uint time) => surfaceId is not null && unchecked((int)(time - shownAt)) >= 0;

    private void Stop()
    {
        if (mouseHook != 0) { NativeMethods.UnhookWindowsHookEx(mouseHook); mouseHook = 0; }
        if (foregroundHook != 0) { NativeMethods.UnhookWinEvent(foregroundHook); foregroundHook = 0; }
    }

    public void Dispose() { surfaceId = null; Stop(); }
}
