using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using Sayso.Interop;

namespace Sayso.Platform;

/// <summary>
/// 低级键盘钩子只收集事件，业务异步串行交给 CoreClient，避免超过系统钩子超时。
/// 每个命令有自己的快捷键：单修饰键（如右 Ctrl）或「修饰键 + 普通键」组合（如 Ctrl+T）。
/// 按住模式下按下发 trigger、松开发 stop；按一下模式下按下发 toggle。
/// </summary>
internal sealed class HotkeyMonitor : IDisposable
{
    private readonly NativeMethods.HookProc callback;
    private readonly CoreClient core;
    private readonly Action<Func<Task>> dispatch;
    private readonly Action dismissCandidate;
    private readonly nuint? inputTag;
    private readonly HashSet<uint> pressed = [];
    /// <summary>已触发快捷键的键，松开时一并吞掉。</summary>
    private readonly HashSet<uint> consumed = [];
    private nint hook;
    /// <summary>录制中按下、尚未松开的修饰键：单独松开即录为单修饰键快捷键。</summary>
    private uint? recordingModifier;
    public JsonNode Settings { get; set; } = null!;
    /// <summary>触发已启用命令的快捷键。</summary>
    public IReadOnlySet<(uint Key, ulong Modifiers)> ActiveShortcuts { get; set; } = new HashSet<(uint, ulong)>();
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
            Record(key.Key, down, fresh);
            return 1;
        }
        var hold = Settings["hotkeyStyle"]!.GetValue<string>() == "hold";
        if (!down && consumed.Remove(key.Key))
        {
            if (hold) dispatch(() => core.Call("stop"));
            return 1;
        }
        if (down && fresh && key.Key == 0x1B)
        {
            dispatch(() => core.Call("cancel"));
            // Esc 仍传给前台应用，避免在空闲时吞掉系统按键。
        }
        // 修饰键本身不计入修饰位：单修饰键快捷键要求没有按着其他修饰键。
        var shortcut = (Key: key.Key, Modifiers: Modifiers(key.Key));
        if (down && ActiveShortcuts.Contains(shortcut))
        {
            if (fresh)
            {
                consumed.Add(key.Key);
                var hotkey = new JsonObject { ["keyCode"] = shortcut.Key, ["modifiers"] = shortcut.Modifiers };
                dispatch(() => core.Call(hold ? "trigger" : "toggle", hotkey));
            }
            return 1;
        }
        // Enter 关闭结果框后仍传给前台应用，保留发送消息或换行的原本行为。
        if (down && fresh && key.Key == 0x0D && consumed.Count == 0 && Modifiers(0) == 0 &&
            NativeMethods.GetAsyncKeyState(0x5B) >= 0 && NativeMethods.GetAsyncKeyState(0x5C) >= 0) dismissCandidate();
        return NativeMethods.CallNextHookEx(hook, code, message, data);
    }

    /// <summary>
    /// 录制快捷键：单独按下并松开一个修饰键为单修饰键快捷键；按下「修饰键 + 普通键」为组合快捷键。
    /// 不带修饰键的普通字符会干扰正常打字，只允许功能键单独作为快捷键。
    /// </summary>
    private void Record(uint key, bool down, bool fresh)
    {
        if (key is >= 0xA0 and <= 0xA5)
        {
            if (down) recordingModifier = key;
            else if (recordingModifier == key) Finish(key, 0);
            return;
        }
        if (!down || !fresh) return;
        var modifiers = Modifiers(0);
        if (modifiers != 0 || key is >= 0x70 and <= 0x87) Finish(key, modifiers);
    }

    private void Finish(uint key, ulong modifiers)
    {
        IsRecordingKey = false;
        recordingModifier = null;
        KeyRecorded?.Invoke(key, modifiers);
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
