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
            var saved = await ClipboardService.Snapshot();
            try
            {
                ClipboardService.Copy("Sayso 文字输入 🍎");
                var actual = await Windows.ApplicationModel.DataTransfer.Clipboard.GetContent().GetTextAsync();
                if (actual != "Sayso 文字输入 🍎") throw new Exception("Clipboard content mismatch.");
            }
            finally { Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(saved); }
        });
        await Check("Bitmap clipboard preserves its raw image while pasting into a real input", ClipboardChecks.InsertOverBitmap);
        await Check("Core exposes all services and builtin extensions", async () =>
        {
            if ((await core.Call("asrSources"))!.AsArray().Count != 4) throw new Exception("Expected four ASR services.");
            if ((await core.Call("extensions"))!.AsArray().Count != 3) throw new Exception("Expected three builtin extensions.");
            if ((await core.Call("languages"))!.AsArray().Count != 8) throw new Exception("Expected eight languages.");
            await core.Call("setEnabled", new JsonObject { ["id"] = "sayso.wordbook", ["enabled"] = false });
            if ((await core.Call("extensions"))!.AsArray().Single(e => e!["manifest"]!["id"]!.GetValue<string>() == "sayso.wordbook")!["enabled"]!.GetValue<bool>()) throw new Exception("Extension was not disabled.");
            await core.Call("setEnabled", new JsonObject { ["id"] = "sayso.wordbook", ["enabled"] = true });
        });
        await Check("Core routing preserves selection input, shared routes and unbinding", async () =>
        {
            var original = (await core.Call("settings"))!;
            var route = new JsonObject { ["kind"] = "key", ["code"] = 89 };
            JsonObject Binding(string extension, bool bound = true) => new()
            {
                ["command"] = new JsonObject { ["extensionId"] = extension, ["commandId"] = "translate" },
                ["route"] = bound ? route.DeepClone() : null
            };
            try
            {
                var updated = (await core.Call("bindCommand", Binding("sayso.selection")))!;
                var selectedRoute = updated["routes"]!.AsArray().Single(r => JsonNode.DeepEquals(r!["key"], route))!;
                if (selectedRoute["input"]!.GetValue<string>() != "selection") throw new Exception("Rebinding lost the selection input source.");
                await core.Call("bindCommand", Binding("sayso.dictation"));
                updated = (await core.Call("setRouteInput", new JsonObject { ["route"] = route.DeepClone(), ["input"] = "speech" }))!;
                if (updated["commandBindings"]!.AsArray().Count(b => JsonNode.DeepEquals(b!["route"], route)) != 2) throw new Exception("Commands did not share the route.");
                if (updated["routes"]!.AsArray().Single(r => JsonNode.DeepEquals(r!["key"], route))!["input"]!.GetValue<string>() != "speech") throw new Exception("Route input was not updated.");
                updated = (await core.Call("bindCommand", Binding("sayso.selection", false)))!;
                if (!updated["routes"]!.AsArray().Any(r => JsonNode.DeepEquals(r!["key"], route))) throw new Exception("A shared route was removed prematurely.");
                updated = (await core.Call("bindCommand", Binding("sayso.dictation", false)))!;
                if (updated["routes"]!.AsArray().Any(r => JsonNode.DeepEquals(r!["key"], route))) throw new Exception("An unused route was retained.");
            }
            finally { await core.Call("updateSettings", original); }
        });
        foreach (var page in new[] { "general", "models", "extensions", "sayso.wordbook" })
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
            await core.Call("trigger", new JsonObject { ["kind"] = "key", ["code"] = 0x54 });
            await core.Call("cancel");
            await Task.Delay(100);
            if ((await core.Call("session"))!["kind"]!.GetValue<string>() != "idle") throw new Exception("Selection cancellation failed.");
        });
        var inspectRecording = Environment.GetCommandLineArgs().Contains("--inspect-recording");
        var inspect = inspectRecording || Environment.GetCommandLineArgs().Contains("--inspect-candidate");
        await Check("Missing service configuration produces a compact nonactivating candidate", async () =>
        {
            await core.Call("trigger", new JsonObject { ["kind"] = "direct" });
            await Task.Delay(200);
            if ((await core.Call("session"))!["kind"]!.GetValue<string>() != "presenting") throw new Exception("Expected a configuration error surface.");
            candidate.ValidateCompactNotice();
            await core.Call("dismiss");
        });
        await Check("Korean speech uses its locale and reports a missing system voice in the candidate", async () =>
        {
            using var installed = new System.Speech.Synthesis.SpeechSynthesizer();
            var hasKorean = installed.GetInstalledVoices().Any(voice => voice.Enabled && voice.VoiceInfo.Culture.TwoLetterISOLanguageName == "ko");
            await core.Call("trigger", new JsonObject { ["kind"] = "direct" });
            var surface = (await core.Call("session"))!;
            await core.Call("candidateAction", new JsonObject
            {
                ["surface"] = surface["id"]!.GetValue<string>(), ["source"] = "sayso.app", ["block"] = "status",
                ["action"] = new JsonObject { ["type"] = "speak", ["text"] = "괜찮아", ["language"] = "ko" }
            });
            var result = (await core.Call("session"))!;
            var notice = result["sections"]!.AsArray().SelectMany(section => section!["blocks"]!.AsArray())
                .SingleOrDefault(block => block!["id"]!.GetValue<string>() == "speech");
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
            ["kind"] = "recording", ["text"] = text, ["commands"] = new JsonArray("听写")
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
                {"kind":"presenting","id":"layout-test","working":false,"sections":[{"sourceId":"sayso.dictation","blocks":[{"id":"text","type":"text","runs":[{"text":"The speaker speaks English and Chinese. A English sentence may contain Chinese words, and a Chinese sentence may contain English words; transcribe every word in the language it was spoken, without translating."}]}]}]}
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
                {"kind":"presenting","id":"interaction-test","working":false,"sections":[{"sourceId":"sayso.dictation","blocks":[{"id":"text","type":"text","runs":[{"text":"こんにちは。","style":"highlight","action":"s1"},{"text":"わかりました。"},{"text":"動物園に行きます。","style":"highlight","action":"s2"}],"actions":[{"id":"s1","type":"speak","text":"こんにちは。","language":"ja"},{"id":"s2","type":"speak","text":"動物園に行きます。","language":"ja"}]},{"id":"term1","type":"term","term":"こんにちは。","language":"ja","gloss":"你好。","actions":[{"type":"invoke","label":"加入生词本","payload":[]}]},{"id":"term2","type":"term","term":"週末は家族と一緒に動物園に行って、いろいろな動物を見たいと思っています。","language":"ja","gloss":"周末我想和家人一起去动物园，看看各种各样的动物。","actions":[{"type":"invoke","label":"加入生词本","payload":[]}]}]}]}
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
                var speakerOrigin = speakers[index].TransformToVisual(content).TransformPoint(new(0, 0));
                var saveOrigin = saves[index].TransformToVisual(content).TransformPoint(new(0, 0));
                if (Math.Abs(speakerOrigin.Y + speakers[index].ActualHeight / 2 - saveOrigin.Y - saves[index].ActualHeight / 2) > 1)
                    throw new Exception("The term speaker and save action must share one row.");
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
                                ["id"] = "text", ["type"] = "text", ["runs"] = new JsonArray(
                                    new JsonObject { ["text"] = sample[..5] },
                                    new JsonObject { ["text"] = sample[5..], ["style"] = "highlight", ["action"] = "play" }),
                                ["actions"] = new JsonArray(new JsonObject { ["id"] = "play", ["type"] = "speak", ["text"] = sample[5..], ["language"] = "ja" })
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
