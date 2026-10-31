using System.IO.Pipes;
using System.Text.Json;
using Microsoft.UI.Dispatching;

namespace Sayso.Platform;

/// <summary>二次启动把扩展包路径交给已有实例，双击安装与开始菜单启动共用同一入口。</summary>
internal sealed class SingleInstance : IDisposable
{
    private readonly Mutex mutex;
    private readonly string pipeName;
    private readonly CancellationTokenSource lifetime = new();
    public bool IsFirst { get; }

    public SingleInstance(bool isolated)
    {
        pipeName = isolated ? "Sayso.Windows.Smoke." + Environment.ProcessId : "Sayso.Windows." + Environment.UserName;
        mutex = new Mutex(true, "Local\\" + pipeName, out var first);
        IsFirst = first;
    }

    public async Task Notify(string? package)
    {
        using var client = new NamedPipeClientStream(".", pipeName, PipeDirection.Out, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        await client.ConnectAsync(15000);
        using var writer = new StreamWriter(client) { AutoFlush = true };
        await writer.WriteLineAsync(JsonSerializer.Serialize(package));
    }

    public void Listen(DispatcherQueue dispatcher, Action<string?> received) => _ = Task.Run(async () =>
    {
        while (!lifetime.IsCancellationRequested)
        {
            try
            {
                using var server = new NamedPipeServerStream(pipeName, PipeDirection.In, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
                await server.WaitForConnectionAsync(lifetime.Token);
                using var reader = new StreamReader(server);
                var line = await reader.ReadLineAsync(lifetime.Token);
                if (line is not null)
                {
                    var package = JsonSerializer.Deserialize<string?>(line);
                    dispatcher.TryEnqueue(() => received(package));
                }
            }
            catch (OperationCanceledException) { break; }
            catch (Exception error) { App.Log(error); }
        }
    });

    public void Dispose() { lifetime.Cancel(); mutex.Dispose(); }
}
