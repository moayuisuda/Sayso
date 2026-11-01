namespace Sayso.Platform;

/// <summary>在 UI STA 线程使用系统剪贴板；只有剪贴板仍属于本次操作时才恢复快照。</summary>
internal static class ClipboardService
{
    public static Task<NativeClipboard.Version> Copy(string text) => NativeClipboard.WriteText(text);
    internal static Task<NativeClipboard.Snapshot> Snapshot(CancellationToken cancellation = default) => NativeClipboard.Capture(cancellation);

    public static async Task<string> ReadSelection(CancellationToken cancellation)
    {
        using var original = await Snapshot(cancellation);
        cancellation.ThrowIfCancellationRequested();
        var sequence = NativeMethods.GetClipboardSequenceNumber();
        NativeMethods.Chord(0x43);
        for (var i = 0; i < 40; i++)
        {
            await Task.Delay(20, cancellation);
            if (NativeMethods.GetClipboardSequenceNumber() == sequence) continue;
            var (text, copiedSequence) = await NativeClipboard.ReadText(cancellation);
            await original.Restore(copiedSequence);
            return text;
        }
        return "";
    }

    public static async Task<string> Insert(string text, nint destination, CancellationToken cancellation)
    {
        using var original = await Snapshot(cancellation);
        cancellation.ThrowIfCancellationRequested();
        var sequence = await NativeClipboard.WriteText(text, cancellation);
        if (destination == 0) return "copiedToClipboard";
        if (NativeMethods.GetForegroundWindow() != destination && !NativeMethods.SetForegroundWindow(destination)) return "copiedToClipboard";
        await Task.Delay(80, cancellation);
        if (NativeMethods.GetForegroundWindow() != destination) return "copiedToClipboard";
        if (!await NativeClipboard.IsCurrent(sequence, cancellation)) throw new InvalidOperationException("剪贴板已被其他复制操作更新，本次未粘贴。");
        NativeMethods.Chord(0x56);
        await Task.Delay(300);
        await original.Restore(sequence);
        return "inserted";
    }
}
