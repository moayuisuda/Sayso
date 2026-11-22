using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Automation.Provider;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Sayso.Interop;
using Sayso.Platform;
using Sayso.UI;

namespace Sayso;

/// <summary>通过真实录制按钮、低级键盘钩子及设置落盘验证组合键。</summary>
internal static class HotkeyChecks
{
    private const nuint InputTag = SmokeTests.InputTag + 1;

    internal static async Task Recording(CoreClient core, SettingsWindow window)
    {
        var original = (await core.Call("settings"))!;
        var activeMonitor = window.Hotkeys!;
        // 先安装下游钩子，既检测录制事件是否漏出，也防止失败时把测试按键发给用户正在使用的应用。
        using var sink = new RecordingKeySink();
        using var monitor = new HotkeyMonitor(core, window.Run, () => { }, InputTag) { Settings = original };
        window.Hotkeys = monitor;
        try
        {
            foreach (var (keys, expectedKey, expectedModifiers) in new (ushort[], uint, ulong)[]
            {
                ([0xA2, 0x54], 0x54, 1),
                ([0xA3, 0x54], 0x54, 1),
                ([0xA2, 0xA0, 0x54], 0x54, 5),
                ([0xA2, 0xA4, 0x54], 0x54, 3),
                ([0xA3], 0xA3, 0),
                ([0x77], 0x77, 0)
            })
            {
                await window.Navigate("general");
                var recorder = Recorder(window.Content);
                ((IInvokeProvider)new ButtonAutomationPeer(recorder).GetPattern(PatternInterface.Invoke)).Invoke();
                await WaitUntil(() => window.Hotkeys!.IsRecordingKey, "The shortcut recorder did not start.");
                try
                {
                    for (var index = 0; index < keys.Length; index++)
                    {
                        Send(keys[index], false);
                        await Task.Delay(30);
                        if (index < keys.Length - 1 && !window.Hotkeys!.IsRecordingKey)
                            throw new Exception("Recording finished before the combination's main key.");
                    }
                    // 主键长按产生的重复事件与松键事件也属于本次录制，不能触发业务或漏到下游。
                    Send(keys[^1], false);
                }
                finally
                {
                    foreach (var key in keys.Reverse()) Send(key, true);
                }
                await Task.Delay(30);
                await WaitUntil(() => !window.Hotkeys!.IsRecordingKey &&
                    !ReferenceEquals(Recorder(window.Content), recorder) &&
                    Recorder(window.Content).Content as string == HotkeyMonitor.KeyName(expectedKey, expectedModifiers),
                    $"The recorder did not display {HotkeyMonitor.KeyName(expectedKey, expectedModifiers)}.");
                var disk = JsonNode.Parse(await File.ReadAllTextAsync(Path.Combine(App.DataDirectory, "settings.json")))!;
                var saved = disk["shortcuts"]!.AsArray().Single(item => item!["input"]!.GetValue<string>() == "selection")!["key"]!;
                if (saved["keyCode"]!.GetValue<uint>() != expectedKey || saved["modifiers"]!.GetValue<ulong>() != expectedModifiers)
                    throw new Exception($"The recorded shortcut was not persisted correctly: {saved}.");
                if (sink.LeakedKeys.Count != 0)
                    throw new Exception($"Recording leaked keys to downstream hooks: {string.Join(", ", sink.LeakedKeys)}.");
                if ((await core.Call("session"))!["kind"]!.GetValue<string>() != "idle")
                    throw new Exception("Recording triggered a Sayso session.");
            }
        }
        finally
        {
            monitor.IsRecordingKey = false;
            monitor.KeyRecorded = null;
            window.Hotkeys = activeMonitor;
            await core.Call("cancel");
            await core.Call("updateSettings", original);
            await window.ReloadSettings();
            await window.Navigate("general");
        }
    }

    private static Button Recorder(DependencyObject parent)
    {
        ((FrameworkElement)parent).UpdateLayout();
        return Find(parent) ?? throw new Exception("The selection shortcut recorder is missing.");

        static Button? Find(DependencyObject item)
        {
            if (item is Button button && AutomationProperties.GetAutomationId(button) == "ShortcutRecorder-selection") return button;
            for (var index = 0; index < VisualTreeHelper.GetChildrenCount(item); index++)
                if (Find(VisualTreeHelper.GetChild(item, index)) is { } found) return found;
            return null;
        }
    }

    private static async Task WaitUntil(Func<bool> condition, string error)
    {
        var deadline = DateTime.UtcNow.AddSeconds(3);
        while (!condition())
        {
            if (DateTime.UtcNow >= deadline) throw new Exception(error);
            await Task.Delay(20);
        }
    }

    private static void Send(ushort key, bool up)
    {
        NativeMethods.Input[] input = [new() { Type = 1, Data = new() { Keyboard = new()
        {
            Key = key, Flags = (up ? 2u : 0u) | (key is 0xA3 or 0xA5 ? 1u : 0u), Extra = InputTag
        } } }];
        if (NativeMethods.SendInput(1, input, Marshal.SizeOf<NativeMethods.Input>()) != 1)
            throw new System.ComponentModel.Win32Exception();
    }

    private sealed class RecordingKeySink : IDisposable
    {
        private readonly NativeMethods.HookProc callback;
        private readonly nint hook;
        internal List<uint> LeakedKeys { get; } = [];

        internal RecordingKeySink()
        {
            callback = OnKeyboard;
            hook = NativeMethods.SetWindowsHookExW(13, callback, NativeMethods.GetModuleHandle(null), 0);
            if (hook == 0) throw new System.ComponentModel.Win32Exception();
        }

        private nint OnKeyboard(int code, nint message, nint data)
        {
            if (code >= 0)
            {
                var key = Marshal.PtrToStructure<NativeMethods.KeyboardEvent>(data);
                if (key.Extra == InputTag)
                {
                    LeakedKeys.Add(key.Key);
                    return 1;
                }
            }
            return NativeMethods.CallNextHookEx(hook, code, message, data);
        }

        public void Dispose() => NativeMethods.UnhookWindowsHookEx(hook);
    }
}
