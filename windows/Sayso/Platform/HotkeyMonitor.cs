using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using Sayso.Interop;

namespace Sayso.Platform;

/// <summary>低级键盘钩子只收集事件，业务异步串行交给 CoreClient，避免超过系统钩子超时。</summary>
internal sealed class HotkeyMonitor : IDisposable
{
    private readonly NativeMethods.HookProc callback;
    private readonly CoreClient core;
    private readonly Action<Func<Task>> dispatch;
    private readonly Action dismissCandidate;
    private readonly nuint? inputTag;
    private readonly HashSet<uint> pressed = [];
    private readonly HashSet<uint> consumedRoutes = [];
    private nint hook;
    private bool held;
    public JsonNode Settings { get; set; } = null!;
    public IReadOnlySet<uint> ActiveRouteKeys { get; set; } = new HashSet<uint>();
    public bool IsRecordingKey { get; set; }
    public Action<uint, ulong>? KeyRecorded { get; set; }

    public HotkeyMonitor(CoreClient core, Action<Func<Task>> dispatch, Action dismissCandidate, nuint? inputTag = null)
    {
        this.core = core;
        this.dispatch = dispatch;
        this.dismissCandidate = dismissCandidate;
        this.inputTag = inputTag;
        callback = OnKeyboard;
        hook = NativeMethods.SetWindowsHookExW(13, callback, NativeMethods.GetModuleHandle(null), 0);
        if (hook == 0) throw new System.ComponentModel.Win32Exception();
    }

    private nint OnKeyboard(int code, nint message, nint data)
    {
        if (code < 0) return NativeMethods.CallNextHookEx(hook, code, message, data);
        var key = Marshal.PtrToStructure<NativeMethods.KeyboardEvent>(data);
        // 隔离集成检查只接收自己的按键，不能抢走用户正在使用的全局快捷键。
        if (inputTag is { } tag && key.Extra != tag) return NativeMethods.CallNextHookEx(hook, code, message, data);
        // 只忽略 Sayso 自己的复制/粘贴按键，保留系统辅助输入和键盘重映射的正常行为。
        if ((key.Flags & 0x10) != 0 && key.Extra == NativeMethods.InjectionTag) return NativeMethods.CallNextHookEx(hook, code, message, data);
        var down = message == 0x100 || message == 0x104;
        var fresh = down ? pressed.Add(key.Key) : pressed.Remove(key.Key);
        if (Settings is null) return NativeMethods.CallNextHookEx(hook, code, message, data);
        if (IsRecordingKey)
        {
            if (down && fresh) { IsRecordingKey = false; KeyRecorded?.Invoke(key.Key, Modifiers(key.Key)); }
            return 1;
        }
        if (!down && consumedRoutes.Remove(key.Key)) return 1;
        var trigger = Settings["triggerKey"]!["keyCode"]!.GetValue<uint>();
        var modifiers = Settings["triggerKey"]!["modifiers"]!.GetValue<ulong>();
        if (key.Key == trigger && (held || Modifiers(trigger) == modifiers))
        {
            if (fresh)
            {
                if (down)
                {
                    held = true;
                    dispatch(() => core.Call(Settings["hotkeyStyle"]!.GetValue<string>() == "hold" ? "trigger" : "toggle",
                        Settings["hotkeyStyle"]!.GetValue<string>() == "hold" ? new JsonObject { ["kind"] = "direct" } : new JsonArray(new JsonObject { ["kind"] = "direct" })));
                }
                else
                {
                    held = false;
                    if (Settings["hotkeyStyle"]!.GetValue<string>() == "hold") dispatch(() => core.Call("stop"));
                }
            }
            return 1;
        }
        if (down && fresh && key.Key == 0x1B)
        {
            dispatch(() => core.Call("cancel"));
            // Esc 仍传给前台应用，避免在空闲时吞掉系统按键。
        }
        if (held && ActiveRouteKeys.Contains(key.Key))
        {
            if (down && fresh)
            {
                consumedRoutes.Add(key.Key);
                dispatch(() => core.Call("trigger", new JsonObject { ["kind"] = "key", ["code"] = key.Key }));
            }
            return 1;
        }
        // Enter 关闭结果框后仍传给前台应用，保留发送消息或换行的原本行为。
        if (down && fresh && key.Key == 0x0D && !held && Modifiers(0) == 0 &&
            NativeMethods.GetAsyncKeyState(0x5B) >= 0 && NativeMethods.GetAsyncKeyState(0x5C) >= 0) dismissCandidate();
        return NativeMethods.CallNextHookEx(hook, code, message, data);
    }

    private static ulong Modifiers(uint excluding)
    {
        ulong mask = 0;
        if (excluding is not (0xA2 or 0xA3) && (NativeMethods.GetAsyncKeyState(0x11) < 0)) mask |= 1;
        if (excluding is not (0xA4 or 0xA5) && (NativeMethods.GetAsyncKeyState(0x12) < 0)) mask |= 2;
        if (excluding is not (0xA0 or 0xA1) && (NativeMethods.GetAsyncKeyState(0x10) < 0)) mask |= 4;
        return mask;
    }

    public static string KeyName(uint code, ulong modifiers = 0)
    {
        var name = code switch { 0xA3 => "右 Ctrl", 0xA2 => "左 Ctrl", 0xA5 => "右 Alt", 0xA4 => "左 Alt", 0xA0 => "左 Shift", 0xA1 => "右 Shift", 0x20 => "Space", _ when code is >= 0x70 and <= 0x87 => $"F{code - 0x6F}", _ when code is >= 0x30 and <= 0x5A => ((char)code).ToString(), _ => $"VK {code:X2}" };
        return ((modifiers & 1) != 0 ? "Ctrl + " : "") + ((modifiers & 2) != 0 ? "Alt + " : "") + ((modifiers & 4) != 0 ? "Shift + " : "") + name;
    }

    public void Dispose() { if (hook != 0) { NativeMethods.UnhookWindowsHookEx(hook); hook = 0; } }
}
