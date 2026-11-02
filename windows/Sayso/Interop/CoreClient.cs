using System.Runtime.InteropServices;
using System.Text.Json.Nodes;

namespace Sayso.Interop;

/// <summary>唯一 Rust core 实例；回调委托在整个原生实例生命周期内保持强引用。</summary>
internal sealed class CoreClient : IDisposable
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate nint Callback(nint request);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void Release(nint value);
    [DllImport("sayso_windows_bridge", CallingConvention = CallingConvention.Cdecl)] private static extern nint sayso_create([MarshalAs(UnmanagedType.LPUTF8Str)] string directory, [MarshalAs(UnmanagedType.LPUTF8Str)] string version, Callback callback, Release release, out nint handle);
    [DllImport("sayso_windows_bridge", CallingConvention = CallingConvention.Cdecl)] private static extern nint sayso_call(nint handle, [MarshalAs(UnmanagedType.LPUTF8Str)] string request);
    [DllImport("sayso_windows_bridge", CallingConvention = CallingConvention.Cdecl)] private static extern void sayso_audio(nint handle, byte[] data, nuint length);
    [DllImport("sayso_windows_bridge", CallingConvention = CallingConvention.Cdecl)] private static extern void sayso_complete(nint handle, ulong id, [MarshalAs(UnmanagedType.LPUTF8Str)] string response);
    [DllImport("sayso_windows_bridge", CallingConvention = CallingConvention.Cdecl)] private static extern void sayso_free(nint value);
    [DllImport("sayso_windows_bridge", CallingConvention = CallingConvention.Cdecl)] private static extern void sayso_destroy(nint handle);

    private nint handle;
    private readonly Callback callback;
    private readonly Release release = Marshal.FreeCoTaskMem;
    private readonly SemaphoreSlim requests = new(1);
    private readonly object lifetime = new();

    public CoreClient(string directory, string version, Func<string, JsonNode?, JsonNode?> platform)
    {
        callback = pointer =>
        {
            JsonObject response;
            try
            {
                var request = JsonNode.Parse(Marshal.PtrToStringUTF8(pointer)!)!;
                response = new() { ["result"] = platform(request["method"]!.GetValue<string>(), request["args"]) };
            }
            catch (Exception error) { response = new() { ["error"] = error.Message }; }
            return Marshal.StringToCoTaskMemUTF8(response.ToJsonString());
        };
        Read(sayso_create(directory, version, callback, release, out handle));
    }

    public async Task<JsonNode?> Call(string method, JsonNode? args = null)
    {
        await requests.WaitAsync();
        try
        {
            ObjectDisposedException.ThrowIf(handle == 0, this);
            // Core 可以同步回调到 UI；始终在线程池上进入原生边界，避免 UI 线程互锁。
            return await Task.Run(() => CallSync(method, args));
        }
        finally { requests.Release(); }
    }

    public async Task<JsonNode> MutateSettings(Action<JsonNode> change)
    {
        await requests.WaitAsync();
        try
        {
            ObjectDisposedException.ThrowIf(handle == 0, this);
            return await Task.Run(() =>
            {
                // 在同一个串行事务中读取与修改，快速操作多个控件时不会互相覆盖旧快照。
                var current = CallSync("settings", null)!;
                change(current);
                return CallSync("updateSettings", current)!;
            });
        }
        finally { requests.Release(); }
    }

    private JsonNode? CallSync(string method, JsonNode? args)
    {
        var request = new JsonObject { ["method"] = method, ["args"] = args?.DeepClone() }.ToJsonString();
        return Read(sayso_call(handle, request));
    }

    public void PushAudio(byte[] data, int count)
    {
        if (handle != 0) sayso_audio(handle, data, (nuint)count);
    }

    public void CompletePlatform(ulong id, JsonObject response)
    {
        lock (lifetime) { if (handle != 0) sayso_complete(handle, id, response.ToJsonString()); }
    }

    private static JsonNode? Read(nint pointer)
    {
        try
        {
            var response = JsonNode.Parse(Marshal.PtrToStringUTF8(pointer)!)!;
            if (response["error"] is { } error) throw new InvalidOperationException(error.GetValue<string>());
            return response["result"]?.DeepClone();
        }
        finally { sayso_free(pointer); }
    }

    public void Dispose()
    {
        requests.Wait();
        try
        {
            lock (lifetime)
            {
                if (handle == 0) return;
                var previous = handle;
                handle = 0;
                sayso_destroy(previous);
            }
        }
        finally { requests.Release(); }
        GC.KeepAlive(callback);
        GC.KeepAlive(release);
    }
}
