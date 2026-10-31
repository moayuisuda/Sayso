using System.Runtime.InteropServices;

namespace Sayso.Platform;

internal static class NativeMethods
{
    internal const uint TrayMessage = 0x8002;
    internal const nuint InjectionTag = 0x5359534F;
    internal delegate nint HookProc(int code, nint message, nint data);
    internal delegate void WinEventProc(nint hook, uint eventType, nint window, int objectId, int childId, uint thread, uint time);
    internal delegate nint SubclassProc(nint hwnd, uint message, nuint wparam, nint lparam, nuint id, nuint data);
    [StructLayout(LayoutKind.Sequential)] internal struct KeyboardEvent { public uint Key, Scan, Flags, Time; public nuint Extra; }
    [StructLayout(LayoutKind.Sequential)] internal struct Input { public uint Type; public InputUnion Data; }
    [StructLayout(LayoutKind.Explicit)] internal struct InputUnion
    {
        [FieldOffset(0)] public KeyboardInput Keyboard;
        [FieldOffset(0)] public MouseInput Mouse;
    }
    [StructLayout(LayoutKind.Sequential)] internal struct KeyboardInput { public ushort Key, Scan; public uint Flags, Time; public nuint Extra; }
    [StructLayout(LayoutKind.Sequential)] internal struct MouseInput { public int X, Y; public uint Data, Flags, Time; public nuint Extra; }
    [StructLayout(LayoutKind.Sequential)] internal struct Point { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] internal struct Rect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] internal struct MouseEvent { public Point Position; public uint Data, Flags, Time; public nuint Extra; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] internal struct NotifyIconData
    {
        public uint Size; public nint Window; public uint Id, Flags, Callback; public nint Icon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string Tip;
        public uint State, StateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string Info;
        public uint Version;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string InfoTitle;
        public uint InfoFlags; public Guid Guid; public nint BalloonIcon;
    }
    [DllImport("user32")] internal static extern nint GetForegroundWindow();
    [DllImport("user32")] internal static extern uint GetDpiForWindow(nint window);
    [DllImport("dwmapi")] private static extern int DwmSetWindowAttribute(nint window, uint attribute, ref uint value, uint size);
    [DllImport("user32")] [return: MarshalAs(UnmanagedType.Bool)] internal static extern bool SetForegroundWindow(nint window);
    [DllImport("user32")] internal static extern uint GetWindowThreadProcessId(nint window, out uint process);
    [DllImport("user32")] internal static extern short GetAsyncKeyState(int key);
    [DllImport("user32", SetLastError = true)] internal static extern uint SendInput(uint count, Input[] inputs, int size);
    [DllImport("user32", SetLastError = true)] internal static extern nint SetWindowsHookExW(int id, HookProc callback, nint module, uint thread);
    [DllImport("user32")] internal static extern bool UnhookWindowsHookEx(nint hook);
    [DllImport("user32")] internal static extern nint CallNextHookEx(nint hook, int code, nint message, nint data);
    [DllImport("user32", SetLastError = true)] internal static extern nint SetWinEventHook(uint min, uint max, nint module, WinEventProc callback, uint process, uint thread, uint flags);
    [DllImport("user32")] internal static extern bool UnhookWinEvent(nint hook);
    [DllImport("user32")] internal static extern bool GetWindowRect(nint window, out Rect rect);
    [DllImport("kernel32", CharSet = CharSet.Unicode)] internal static extern nint GetModuleHandle(string? name);
    [DllImport("user32")] internal static extern uint GetClipboardSequenceNumber();
    [DllImport("user32")] internal static extern nint GetWindowLongPtrW(nint window, int index);
    [DllImport("user32")] internal static extern nint SetWindowLongPtrW(nint window, int index, nint value);
    [DllImport("user32", SetLastError = true)] private static extern bool SetWindowPos(nint window, nint insertAfter, int x, int y, int width, int height, uint flags);
    [DllImport("comctl32")] internal static extern bool SetWindowSubclass(nint window, SubclassProc callback, nuint id, nuint data);
    [DllImport("comctl32")] internal static extern bool RemoveWindowSubclass(nint window, SubclassProc callback, nuint id);
    [DllImport("comctl32")] internal static extern nint DefSubclassProc(nint window, uint message, nuint wparam, nint lparam);
    [DllImport("shell32", CharSet = CharSet.Unicode)] internal static extern bool Shell_NotifyIcon(uint message, ref NotifyIconData data);
    [DllImport("user32", CharSet = CharSet.Unicode)] internal static extern nint LoadImage(nint instance, string path, uint type, int width, int height, uint flags);
    [DllImport("user32")] internal static extern bool DestroyIcon(nint icon);
    [DllImport("user32")] internal static extern nint CreatePopupMenu();
    [DllImport("user32", CharSet = CharSet.Unicode)] internal static extern bool AppendMenu(nint menu, uint flags, nuint id, string text);
    [DllImport("user32")] internal static extern int TrackPopupMenu(nint menu, uint flags, int x, int y, int reserved, nint window, nint rect);
    [DllImport("user32")] internal static extern bool DestroyMenu(nint menu);
    [DllImport("user32")] internal static extern bool GetCursorPos(out Point point);

    internal static void StyleFloatingWindow(nint window)
    {
        // 子类过程已将客户区扩展到整个窗口，此处通知系统重新计算非客户区。
        if (!SetWindowPos(window, 0, 0, 0, 0, 0, 0x0037)) throw new System.ComponentModel.Win32Exception();
        // Windows 11 的系统描边会与内容描边叠加；浮层仅保留主题定义的一条细边。
        if (OperatingSystem.IsWindowsVersionAtLeast(10, 0, 22000))
        {
            uint noBorder = 0xFFFFFFFE, rounded = 2;
            Marshal.ThrowExceptionForHR(DwmSetWindowAttribute(window, 34, ref noBorder, sizeof(uint)));
            Marshal.ThrowExceptionForHR(DwmSetWindowAttribute(window, 33, ref rounded, sizeof(uint)));
        }
    }

    internal static void Chord(ushort key)
    {
        var input = new List<Input>();
        // 路由触发时入口修饰键仍可能按住；先释放 OS 的修饰状态，注入事件由热键钩子忽略。
        foreach (ushort modifier in new ushort[] { 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C })
            if (GetAsyncKeyState(modifier) < 0) input.Add(Key(modifier, true));
        input.AddRange([Key(0xA2), Key(key), Key(key, true), Key(0xA2, true)]);
        var sent = SendInput((uint)input.Count, input.ToArray(), Marshal.SizeOf<Input>());
        if (sent != input.Count) throw new InvalidOperationException("无法向目标应用发送按键，请确认目标应用未以管理员权限运行。");
    }

    private static Input Key(ushort key, bool up = false) => new() { Type = 1, Data = new() { Keyboard = new() { Key = key, Flags = up ? 2u : 0u, Extra = InjectionTag } } };
}
