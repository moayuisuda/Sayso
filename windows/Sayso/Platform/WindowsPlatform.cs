using System.Diagnostics;
using System.Collections.Concurrent;
using System.Speech.Synthesis;
using System.Text.Json.Nodes;
using Microsoft.UI.Dispatching;
using Sayso.Interop;

namespace Sayso.Platform;

internal sealed class WindowsPlatform(DispatcherQueue dispatcher) : IDisposable
{
    private readonly SpeechSynthesizer speech = new();
    private readonly object speechLock = new();
    private AudioRecorder? audio;
    public float AudioLevel => audio?.Level ?? 0;
    private readonly ConcurrentDictionary<ulong, CancellationTokenSource> operations = new();
    public event Action<string, JsonNode?>? Changed;
    public CoreClient Core { private get; set; } = null!;

    public JsonNode? Handle(string method, JsonNode? args)
    {
        switch (method)
        {
            case "asyncPlatform": BeginOperation(args!); return null;
            case "cancelPlatform":
                if (operations.TryGetValue(args!.GetValue<ulong>(), out var cancelled)) cancelled.Cancel();
                return null;
            case "startAudio":
                audio ??= new AudioRecorder(Core.PushAudio);
                audio.Start();
                return null;
            case "stopAudio": audio?.Stop(); return null;
            case "frontmostApp":
                var window = NativeMethods.GetForegroundWindow();
                NativeMethods.GetWindowThreadProcessId(window, out var process);
                if (process == Environment.ProcessId || window == 0) return null;
                return new JsonObject { ["id"] = window.ToInt64().ToString(), ["name"] = Process.GetProcessById((int)process).ProcessName };
            case "copyText": return OnUI(async () => { await ClipboardService.Copy(args!.GetValue<string>()); return null; }).GetAwaiter().GetResult();
            case "openUrl": Open(args!.GetValue<string>()); return null;
            case "speak":
                Speak(args!["text"]!.GetValue<string>(), args["language"]!.GetValue<string>(), args["rate"]!.GetValue<float>());
                return null;
            case "stopSpeaking": lock (speechLock) speech.SpeakAsyncCancelAll(); return null;
            case "sessionChanged": case "settingsChanged": case "extensionsChanged": case "storeChanged":
                var snapshot = args?.DeepClone();
                dispatcher.TryEnqueue(() => Changed?.Invoke(method, snapshot));
                return null;
            default: throw new InvalidOperationException($"Unknown platform method: {method}");
        }
    }

    private void BeginOperation(JsonNode request)
    {
        var id = request["id"]!.GetValue<ulong>();
        var method = request["method"]!.GetValue<string>();
        var args = request["args"]?.DeepClone();
        var cancellation = new CancellationTokenSource();
        operations[id] = cancellation;
        _ = ExecuteOperation();
        async Task ExecuteOperation()
        {
            JsonObject response;
            try
            {
                var result = await OnUI(async () =>
                {
                    cancellation.Token.ThrowIfCancellationRequested();
                    if (method == "readSelection") return JsonValue.Create(await ClipboardService.ReadSelection(cancellation.Token));
                    if (method == "insertText")
                    {
                        var destination = args!["destination"]?["id"]?.GetValue<string>();
                        var delivery = await ClipboardService.Insert(args["text"]!.GetValue<string>(), destination is null ? 0 : (nint)long.Parse(destination), cancellation.Token);
                        if (delivery == "copiedToClipboard")
                            App.Log($"Text insertion not performed: destination={destination ?? "none"}, foreground={NativeMethods.GetForegroundWindow()}.");
                        return JsonValue.Create(delivery);
                    }
                    throw new InvalidOperationException($"Unknown asynchronous platform method: {method}");
                });
                response = new() { ["result"] = result };
            }
            catch (Exception error)
            {
                if (error is not OperationCanceledException) App.Log(error);
                response = new() { ["error"] = error.Message };
            }
            if (operations.TryRemove(id, out _)) Core.CompletePlatform(id, response);
            cancellation.Dispose();
        }
    }

    public static void Open(string path) => Process.Start(new ProcessStartInfo(path) { UseShellExecute = true });

    public void Speak(string text, string locale, float rate)
    {
        lock (speechLock)
        {
            speech.SpeakAsyncCancelAll();
            var voice = speech.GetInstalledVoices().FirstOrDefault(v => v.Enabled && v.VoiceInfo.Culture.Name.StartsWith(locale.Split('-')[0], StringComparison.OrdinalIgnoreCase));
            if (voice is null) throw new InvalidOperationException($"Windows 未安装 {locale} 朗读语音。请在「设置 → 时间和语言 → 语音 → 添加语音」安装对应语音，完成后重启 Sayso。");
            speech.SelectVoice(voice.VoiceInfo.Name);
            speech.Rate = (int)Math.Clamp((rate - 0.5f) * 20, -10, 10);
            speech.SetOutputToDefaultAudioDevice();
            speech.SpeakAsync(text);
        }
    }

    private Task<JsonNode?> OnUI(Func<Task<JsonNode?>> action)
    {
        var completion = new TaskCompletionSource<JsonNode?>(TaskCreationOptions.RunContinuationsAsynchronously);
        if (!dispatcher.TryEnqueue(async () =>
        {
            try { completion.SetResult(await action()); }
            catch (Exception error) { completion.SetException(error); }
        })) completion.SetException(new InvalidOperationException("UI dispatcher has stopped."));
        return completion.Task;
    }

    public void Stop()
    {
        audio?.Stop();
        foreach (var operation in operations.Values) operation.Cancel();
    }
    public void Dispose() { audio?.Dispose(); lock (speechLock) speech.Dispose(); }
}
