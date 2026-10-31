using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using Sayso.Interop;
using Sayso.Platform;
using Sayso.UI;

namespace Sayso;

/// <summary>用真实键鼠钩子与 Core 会话验证关闭策略，按键和点击必须继续送达输入窗口。</summary>
internal static class CandidateChecks
{
    internal static async Task Dismissal(CoreClient core, CandidateWindow candidate)
    {
        using var target = new InputTestWindow();
        await target.Show();
        var modifiersDeadline = DateTime.UtcNow.AddSeconds(10);
        while (new[] { 0x10, 0x11, 0x12, 0x5B, 0x5C }.Any(key => NativeMethods.GetAsyncKeyState(key) < 0))
        {
            if (DateTime.UtcNow >= modifiersDeadline) throw new Exception("Modifiers remained held during the isolated Enter check.");
            await Task.Delay(50);
        }
        var first = await Present();
        await core.Call("dismissSurface", JsonValue.Create("stale-surface"));
        await Require("presenting");

        if (NativeMethods.GetForegroundWindow() != target.Handle) throw new Exception("Input focus changed before the Enter check.");
        Send([
            new() { Type = 1, Data = new() { Keyboard = new() { Key = 0x0D, Extra = SmokeTests.InputTag } } },
            new() { Type = 1, Data = new() { Keyboard = new() { Key = 0x0D, Flags = 2, Extra = SmokeTests.InputTag } } }
        ]);
        await Require("idle");
        var enterDeadline = DateTime.UtcNow.AddSeconds(2);
        while (!target.Input.Text.Contains('\r') && !target.Input.Text.Contains('\n'))
        {
            if (DateTime.UtcNow >= enterDeadline) throw new Exception($"Closing the candidate swallowed Enter: focus={target.Input.FocusState}, foreground={NativeMethods.GetForegroundWindow()}, target={target.Handle}.");
            await Task.Delay(20);
        }

        NativeMethods.GetCursorPos(out var originalCursor);
        try
        {
            await Present();
            NativeMethods.GetWindowRect(WinRT.Interop.WindowNative.GetWindowHandle(candidate), out var bounds);
            Click(bounds.Left + 12, bounds.Top + 12);
            await Task.Delay(100);
            await Require("presenting");
            if (!NativeMethods.GetWindowRect(target.Handle, out var inputBounds)) throw new Exception("Missing input window bounds.");
            Click(inputBounds.Left + 40, inputBounds.Top + 90);
            await Require("idle");
        }
        finally { SetCursorPos(originalCursor.X, originalCursor.Y); }

        // 前台窗口切换不依赖鼠标；用同一进程中的第二个真实窗口覆盖 Alt+Tab 一类场景。
        using var other = new InputTestWindow();
        await other.Show();
        await target.Show();
        await Present();
        await other.Show();
        await Require("idle");
        await core.Call("dismissSurface", JsonValue.Create(first));

        async Task<string> Present()
        {
            await core.Call("trigger", new JsonObject { ["kind"] = "direct" });
            var session = (await core.Call("session"))!;
            if (session["kind"]!.GetValue<string>() != "presenting") throw new Exception("Expected the missing-configuration candidate.");
            await Task.Delay(100);
            return session["id"]!.GetValue<string>();
        }
        async Task Require(string kind)
        {
            var deadline = DateTime.UtcNow.AddSeconds(2);
            while (true)
            {
                var actual = (await core.Call("session"))!["kind"]!.GetValue<string>();
                if (actual == kind) return;
                if (DateTime.UtcNow >= deadline) throw new Exception($"Expected {kind}, got {actual}.");
                await Task.Delay(20);
            }
        }
    }

    private static void Click(int x, int y)
    {
        if (!SetCursorPos(x, y)) throw new System.ComponentModel.Win32Exception();
        Send([
            new() { Type = 0, Data = new() { Mouse = new() { Flags = 2 } } },
            new() { Type = 0, Data = new() { Mouse = new() { Flags = 4 } } }
        ]);
    }

    private static void Send(NativeMethods.Input[] input)
    {
        if (NativeMethods.SendInput((uint)input.Length, input, Marshal.SizeOf<NativeMethods.Input>()) != input.Length)
            throw new System.ComponentModel.Win32Exception();
    }

    [DllImport("user32", SetLastError = true)] private static extern bool SetCursorPos(int x, int y);
}
