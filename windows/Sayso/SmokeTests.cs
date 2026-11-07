using System.Text.Json.Nodes;
using Sayso.Interop;
using Sayso.Platform;
using Sayso.UI;

namespace Sayso;

/// <summary>在实际 WinUI 进程、原生 DLL 与 Windows API 上执行集成检查，独立数据目录不污染用户设置。</summary>
internal static class SmokeTests
{
    internal const nuint InputTag = 0x53595453;
    internal static async Task Run(CoreClient core, WindowsPlatform platform, SettingsWindow window, CandidateWindow candidate, Action quit)
    {
        if (Environment.GetCommandLineArgs().Contains("--wait-for-foreground"))
        {
            // 自动检查启动器可能不拥有前台权限；先让交互桌面显式激活测试窗口，再注入键鼠。
            var handle = WinRT.Interop.WindowNative.GetWindowHandle(window);
            var deadline = DateTime.UtcNow.AddSeconds(30);
            while (NativeMethods.GetForegroundWindow() != handle)
            {
                if (DateTime.UtcNow >= deadline) throw new InvalidOperationException("请在交互桌面激活 Sayso 测试窗口后运行集成检查。");
                await Task.Delay(50);
            }
        }
        var results = new JsonArray();
        async Task Check(string name, Func<Task> action)
        {
            try { await action(); results.Add(new JsonObject { ["name"] = name, ["passed"] = true }); }
            catch (Exception error) { results.Add(new JsonObject { ["name"] = name, ["passed"] = false, ["error"] = error.ToString() }); }
        }
        await Check("Core settings persist through the native ABI", async () =>
        {
            var original = (await core.Call("settings"))!;
            var updated = original.DeepClone(); updated["targetLanguage"] = "ja";
            await core.Call("updateSettings", updated);
            var disk = JsonNode.Parse(await File.ReadAllTextAsync(Path.Combine(App.DataDirectory, "settings.json")))!;
            if (disk["targetLanguage"]!.GetValue<string>() != "ja") throw new Exception("Settings were not persisted.");
            await core.Call("updateSettings", original);
        });
        await Check("Windows clipboard Unicode roundtrip and restoration", async () =>
        {
            using var saved = await ClipboardService.Snapshot();
            try
            {
                await ClipboardService.Copy("Sayso 文字输入 🍎");
                var (actual, _) = await NativeClipboard.ReadText();
                if (actual != "Sayso 文字输入 🍎") throw new Exception("Clipboard content mismatch.");
            }
            finally { await saved.Restore(); }
        });
        await Check("Native clipboard preserves text, file lists and binary formats without overwriting newer copies", ClipboardChecks.MixedFormatsAndConcurrentCopy);
        await Check("Bitmap clipboard preserves its raw image while pasting into a real input", ClipboardChecks.InsertOverBitmap);
        await Check("Core exposes all services and builtin extensions", async () =>
        {
            if ((await core.Call("asrSources"))!.AsArray().Count != 4) throw new Exception("Expected four ASR services.");
            if ((await core.Call("extensions"))!.AsArray().Count != 4) throw new Exception("Expected four builtin extensions.");
            if ((await core.Call("languages"))!.AsArray().Count != 8) throw new Exception("Expected eight languages.");
            await core.Call("setEnabled", new JsonObject { ["id"] = "sayso.wordbook", ["enabled"] = false });
            if ((await core.Call("extensions"))!.AsArray().Single(e => e!["manifest"]!["id"]!.GetValue<string>() == "sayso.wordbook")!["enabled"]!.GetValue<bool>()) throw new Exception("Extension was not disabled.");
            await core.Call("setEnabled", new JsonObject { ["id"] = "sayso.wordbook", ["enabled"] = true });
        });
        await Check("Portable shell runs through the native ABI and packaged helper", async () =>
        {
            var directory = Path.Combine(App.DataDirectory, "Extensions", "smoke-shell");
            Directory.CreateDirectory(directory);
            await File.WriteAllTextAsync(Path.Combine(directory, "extension.json"), """
                {"schemaVersion":4,"id":"test.smoke.shell","name":"Shell check","main":"main.js",
                 "events":["focus"],"capabilities":["shell","store"],"timeoutSeconds":10}
                """);
            await File.WriteAllTextAsync(Path.Combine(directory, "main.js"), """
                export async function focus() {
                  const result = await sayso.shell.run("cat | head -n 1; echo issue >&2; exit 7", {
                    stdin: "Sayso 中文 🍎\nsecond line\n", timeoutMs: 5000
                  });
                  sayso.store.put("checks", "shell", result);
                }
                """);
            await core.Call("reloadExtensions");
            await core.Call("setEnabled", new JsonObject { ["id"] = "test.smoke.shell", ["enabled"] = true });
            try
            {
                await core.Call("appFocused", new JsonObject { ["id"] = "sayso-shell-check", ["name"] = "Shell check" });
                var report = Path.Combine(App.DataDirectory, "ExtensionData", "test.smoke.shell", "checks.json");
                var deadline = DateTime.UtcNow.AddSeconds(10);
                while (!File.Exists(report))
                {
                    if (DateTime.UtcNow >= deadline) throw new Exception("The packaged shell helper did not complete.");
                    await Task.Delay(50);
                }
                var result = JsonNode.Parse(await File.ReadAllTextAsync(report))!.AsArray().Single()!;
                if (result["exitCode"]!.GetValue<int>() != 7 || result["stdout"]!.GetValue<string>() != "Sayso 中文 🍎\n" || result["stderr"]!.GetValue<string>() != "issue\n")
                    throw new Exception("The packaged shell result did not survive the JS/native boundary.");
            }
            finally { await core.Call("setEnabled", new JsonObject { ["id"] = "test.smoke.shell", ["enabled"] = false }); }
        });
        await Check("Core shortcuts keep input on rekey, move between commands and clear", async () =>
        {
            var original = (await core.Call("settings"))!;
            var key = new JsonObject { ["keyCode"] = 89, ["modifiers"] = 1 };
            JsonObject Shortcut(string extension, bool set = true) => new()
            {
                ["command"] = new JsonObject { ["extensionId"] = extension, ["commandId"] = "translate" },
                ["key"] = set ? key.DeepClone() : null
            };
            JsonNode? At(JsonNode settings) => settings["shortcuts"]!.AsArray().SingleOrDefault(s => JsonNode.DeepEquals(s!["key"], key));
            try
            {
                var updated = (await core.Call("setShortcut", Shortcut("sayso.selection")))!;
                if (At(updated)?["input"]!.GetValue<string>() != "selection") throw new Exception("Rekeying lost the selection input source.");
                updated = (await core.Call("setShortcut", Shortcut("sayso.dictation")))!;
                if (At(updated)?["command"]!["extensionId"]!.GetValue<string>() != "sayso.dictation") throw new Exception("The shortcut did not move to the new command.");
                if (updated["shortcuts"]!.AsArray().Any(s => s!["command"]!["extensionId"]!.GetValue<string>() == "sayso.selection")) throw new Exception("The previous command kept the shortcut.");
                updated = (await core.Call("setShortcut", Shortcut("sayso.dictation", false)))!;
                if (At(updated) is not null) throw new Exception("A cleared shortcut was retained.");
            }
            finally { await core.Call("updateSettings", original); }
        });
        foreach (var page in new[] { "general", "models", "extensions", "sayso.wordbook", "sayso.context" })
            await Check("WinUI page and layout: " + page, async () => { await window.Navigate(page); await Task.Delay(100); window.ValidateLayout(); });
        await Check("WASAPI capture and 16 kHz PCM conversion", async () =>
        {
            var received = 0;
            using var recorder = new AudioRecorder((_, length) => Interlocked.Add(ref received, length));
            await Task.Run(recorder.Start);
            await Task.Delay(500);
            await Task.Run(recorder.Stop);
            if (received == 0 || received % 2 != 0) throw new Exception($"Invalid PCM delivery: {received} bytes.");
        });
        await Check("Cancelling a selection propagates across the asynchronous native boundary", async () =>
        {
            await core.Call("trigger", new JsonObject { ["keyCode"] = 0x54, ["modifiers"] = 1 });
            await core.Call("cancel");
            await Task.Delay(100);
            if ((await core.Call("session"))!["kind"]!.GetValue<string>() != "idle") throw new Exception("Selection cancellation failed.");
        });
        var inspectRecording = Environment.GetCommandLineArgs().Contains("--inspect-recording");
        var inspectGlow = Environment.GetCommandLineArgs().Contains("--inspect-glow");
        var inspect = inspectRecording || inspectGlow || Environment.GetCommandLineArgs().Contains("--inspect-candidate");
        await Check("Missing service configuration produces a compact nonactivating candidate", async () =>
        {
            await core.Call("trigger", new JsonObject { ["keyCode"] = 0xA3, ["modifiers"] = 0 });
            await Task.Delay(200);
            if ((await core.Call("session"))!["kind"]!.GetValue<string>() != "presenting") throw new Exception("Expected a configuration error surface.");
            candidate.ValidateCompactNotice();
            await core.Call("dismiss");
        });
        await Check("Korean speech uses its locale and reports a missing system voice in the candidate", async () =>
        {
            using var installed = new System.Speech.Synthesis.SpeechSynthesizer();
            var hasKorean = installed.GetInstalledVoices().Any(voice => voice.Enabled && voice.VoiceInfo.Culture.TwoLetterISOLanguageName == "ko");
            await core.Call("trigger", new JsonObject { ["keyCode"] = 0xA3, ["modifiers"] = 0 });
            var surface = (await core.Call("session"))!;
            await core.Call("candidateAction", new JsonObject
            {
                ["surface"] = surface["id"]!.GetValue<string>(), ["source"] = "sayso.app", ["block"] = "status",
                ["action"] = new JsonObject { ["type"] = "speak", ["text"] = "괜찮아", ["language"] = "ko" }
            });
            var result = (await core.Call("session"))!;
            var notice = result["sections"]!.AsArray().SelectMany(section => section!["blocks"]!.AsArray())
                .Select(item => item!["block"]).SingleOrDefault(block => block!["id"]!.GetValue<string>() == "speech");
            if (hasKorean && notice is not null) throw new Exception("An installed Korean voice failed: " + notice.ToJsonString());
            if (!hasKorean && notice?["message"]?.GetValue<string>().Contains("ko-KR") != true)
                throw new Exception("A missing Korean voice was silently ignored across the ABI.");
            await Task.Delay(100);
            candidate.ValidateContentSurface();
            await core.Call("dismiss");
        });
        await Check("Candidate dismisses on Enter, outside click and foreground changes without consuming input", async () =>
        {
            try { await CandidateChecks.Dismissal(core, candidate); }
            finally { await core.Call("dismiss"); await Task.Delay(50); }
        });
        JsonObject Recording(string text = "") => new()
        {
            ["kind"] = "recording", ["text"] = text, ["command"] = "听写"
        };
        await Check("Live microphone feedback preserves time and stops after recording", async () =>
        {
            // 使用真实 WASAPI 输入与正式平台的电平源；会话帧只用于隔离验证视图生命周期。
            try
            {
                await Task.Run(() => platform.Handle("startAudio", null));
                candidate.Update(Recording());
                await Task.Delay(1100);
                var first = candidate.ValidateRecordingFeedback(TimeSpan.FromSeconds(1));
                candidate.Update(Recording("正在输入的识别文字"));
                await Task.Delay(100);
                candidate.ValidateRecordingFeedback(TimeSpan.FromSeconds(1));
                if (first.IsUpdating) throw new Exception("A replaced recording indicator retained its timer.");
                await Task.Run(() => platform.Handle("stopAudio", null));
                var current = candidate.ValidateRecordingFeedback(TimeSpan.FromSeconds(1));
                candidate.Update(new JsonObject { ["kind"] = "processing", ["text"] = "" });
                await Task.Delay(100);
                candidate.ValidateContentSurface();
                if (current.IsUpdating || platform.AudioLevel != 0) throw new Exception("Recording feedback was not cleared after capture stopped.");
                // 流光淡出约 0.25 秒后必须停止逐帧刷新。
                await Task.Delay(400);
                if (candidate.RecordingGlowAnimating) throw new Exception("The recording glow kept rendering after recording ended.");
            }
            finally
            {
                await Task.Run(() => platform.Handle("stopAudio", null));
                candidate.Update(new JsonObject { ["kind"] = "idle" });
            }
        });
        await Check("Multiline candidate surface fills the window", async () =>
        {
            // 覆盖实际结果的多行布局，避免只验证紧凑错误提示而漏掉内容表面的边缘。
            candidate.Update(JsonNode.Parse("""
                {"kind":"presenting","id":"layout-test","working":false,"sections":[{"sourceId":"sayso.dictation","blocks":[{"block":{"id":"text","type":"text","runs":[{"text":"The speaker speaks English and Chinese. A English sentence may contain Chinese words, and a Chinese sentence may contain English words; transcribe every word in the language it was spoken, without translating."}]},"attachments":[]}]}]}
                """)!);
            await Task.Delay(100);
            candidate.ValidateContentSurface();
            if (inspect)
            {
                candidate.PrepareForInspection();
            }
            else candidate.Update(new JsonObject { ["kind"] = "idle" });
        });
        await Check("Candidate keeps speech controls stable on hover and hides inline action duplicates", async () =>
        {
            var frame = JsonNode.Parse("""
                {"kind":"presenting","id":"interaction-test","working":false,"sections":[{"sourceId":"sayso.dictation","blocks":[{"block":{"id":"text","type":"text","runs":[{"text":"こんにちは。","style":"highlight","action":"s1"},{"text":"わかりました。"},{"text":"動物園に行きます。","style":"highlight","action":"s2"}],"actions":[{"id":"s1","type":"speak","text":"こんにちは。","language":"ja"},{"id":"s2","type":"speak","text":"動物園に行きます。","language":"ja"}]},"attachments":[]},{"block":{"id":"term1","type":"term","term":"こんにちは。","language":"ja","gloss":"你好。"},"attachments":[{"sourceId":"sayso.wordbook","blockRef":"sayso.dictation/term1","actions":[{"type":"invoke","label":"加入生词本","payload":[]}]}]},{"block":{"id":"term2","type":"term","term":"週末は家族と一緒に動物園に行って、いろいろな動物を見たいと思っています。","language":"ja","gloss":"周末我想和家人一起去动物园，看看各种各样的动物。"},"attachments":[{"sourceId":"sayso.wordbook","blockRef":"sayso.dictation/term2","actions":[{"type":"invoke","label":"加入生词本","payload":[]}]}]}]}]}
                """)!;
            candidate.Update(frame);
            await Task.Delay(100);
            var content = candidate.RenderedContent!;
            candidate.Update(frame.DeepClone());
            if (!ReferenceEquals(content, candidate.RenderedContent)) throw new Exception("An unchanged hover notification replaced the clickable controls.");
            var speakers = Descendants(content).OfType<Microsoft.UI.Xaml.Controls.Button>()
                .Where(button => Microsoft.UI.Xaml.Automation.AutomationProperties.GetName(button) == window.L("朗读", "Speak")).ToArray();
            if (speakers.Length != 2) throw new Exception($"Expected two term speakers and no duplicate text buttons; got {speakers.Length}.");
            foreach (var speaker in speakers)
            {
                var bounds = speaker.TransformToVisual(content).TransformBounds(new(0, 0, speaker.ActualWidth, speaker.ActualHeight));
                if (bounds.Right > ((Microsoft.UI.Xaml.FrameworkElement)content).ActualWidth || speaker.ActualWidth < 24)
                    throw new Exception("A speech control was clipped outside the candidate surface.");
            }
            var saves = Descendants(content).OfType<Microsoft.UI.Xaml.Controls.Button>()
                .Where(button => button.Content is string label && label == "加入生词本").ToArray();
            if (saves.Length != 2) throw new Exception("Both translated terms must keep their save action.");
            for (var index = 0; index < speakers.Length; index++)
            {
                // 候选框允许按钮换行，但每个词条的朗读与保存必须属于同一个流式容器。
                var termRow = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(speakers[index]);
                var row = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(termRow);
                if (row is not WrapPanel || !ReferenceEquals(row, Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(saves[index])))
                    throw new Exception("The term speaker and save action must share one wrapping row.");
            }
            candidate.ValidateContentSurface();
            if (inspect) candidate.PrepareForInspection();
            else candidate.Update(new JsonObject { ["kind"] = "idle" });
        });
        await Check("Transcript typography and text origin stay stable through recognition and translation", async () =>
        {
            foreach (var sample in new[] { "Hello, 你好。괜찮아。わかりました。", "はい、わかりました。次は頑張ります。 I will try again next time. 下次我会继续努力。" })
            {
                var frames = new[]
                {
                    Recording(sample),
                    new JsonObject { ["kind"] = "processing", ["text"] = sample },
                    new JsonObject { ["kind"] = "presenting", ["id"] = "typography", ["working"] = true, ["placeholder"] = sample, ["sections"] = new JsonArray() },
                    new JsonObject
                    {
                        ["kind"] = "presenting", ["id"] = "typography", ["working"] = false,
                        ["sections"] = new JsonArray(new JsonObject
                        {
                            ["sourceId"] = "sayso.dictation", ["blocks"] = new JsonArray(new JsonObject
                            {
                                ["block"] = new JsonObject
                                {
                                    ["id"] = "text", ["type"] = "text", ["runs"] = new JsonArray(
                                        new JsonObject { ["text"] = sample[..5] },
                                        new JsonObject { ["text"] = sample[5..], ["style"] = "highlight", ["action"] = "play" }),
                                    ["actions"] = new JsonArray(new JsonObject { ["id"] = "play", ["type"] = "speak", ["text"] = sample[5..], ["language"] = "ja" })
                                },
                                ["attachments"] = new JsonArray()
                            })
                        })
                    }
                };
                (string Font, double Size, ushort Weight, double Line, double Width, double Height, double X, double Y)? baseline = null;
                foreach (var frame in frames)
                {
                    candidate.Update(frame);
                    await Task.Delay(50);
                    var content = candidate.RenderedContent ?? throw new Exception($"Candidate content missing at {frame["kind"]}; core session={(await core.Call("session"))!["kind"]}.");
                    var text = Descendants(content).OfType<Microsoft.UI.Xaml.Controls.RichTextBlock>().Single();
                    var origin = text.TransformToVisual(content).TransformPoint(new(0, 0));
                    var metrics = (text.FontFamily.Source, text.FontSize, text.FontWeight.Weight, text.LineHeight, text.ActualWidth, text.ActualHeight, origin.X, origin.Y);
                    if (baseline is { } previous && metrics != previous)
                        throw new Exception($"Transcript changed layout at {frame["kind"]}: {previous} → {metrics}");
                    baseline = metrics;
                    if (text.Blocks.OfType<Microsoft.UI.Xaml.Documents.Paragraph>().SelectMany(paragraph => paragraph.Inlines)
                        .Any(inline => inline.FontSize != text.FontSize || inline.FontWeight.Weight != text.FontWeight.Weight))
                        throw new Exception("A highlighted or clickable span changed the transcript's type metrics.");
                    candidate.ValidateContentSurface();
                }
            }
            if (inspect) candidate.PrepareForInspection();
            else candidate.Update(new JsonObject { ["kind"] = "idle" });
        });
        if (inspectRecording)
        {
            await Task.Run(() => platform.Handle("startAudio", null));
            candidate.Update(Recording());
            candidate.PrepareForInspection();
        }
        if (inspectGlow)
        {
            candidate.Update(new JsonObject { ["kind"] = "idle" });
            RecordingGlowChecks.Show();
        }
        await File.WriteAllTextAsync(Path.Combine(AppContext.BaseDirectory, "smoke-results.json"), results.ToJsonString(new() { WriteIndented = true }));
        if (!inspect) quit();
    }

    private static IEnumerable<Microsoft.UI.Xaml.DependencyObject> Descendants(Microsoft.UI.Xaml.DependencyObject parent)
    {
        for (var index = 0; index < Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChildrenCount(parent); index++)
        {
            var child = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChild(parent, index);
            yield return child;
            foreach (var descendant in Descendants(child)) yield return descendant;
        }
    }
}
