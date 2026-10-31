using Windows.ApplicationModel.DataTransfer;

namespace Sayso.Platform;

/// <summary>在 UI STA 线程使用系统剪贴板；只有剪贴板仍属于本次操作时才恢复快照。</summary>
internal static class ClipboardService
{
    internal const string DibV5 = "DeviceIndependentBitmapV5";
    public static void Copy(string text)
    {
        var package = new DataPackage();
        package.SetText(text);
        Clipboard.SetContent(package);
        Clipboard.Flush();
    }

    internal static async Task<DataPackage> Snapshot()
    {
        var snapshot = new DataPackage();
        var content = Clipboard.GetContent();
        foreach (var format in content.AvailableFormats)
        {
            // DIBV5 已包含完整图片；Bitmap 是系统派生格式，WinRT 转换某些位域图片时会报内存错误。
            // 保存原始 DIBV5，恢复后 Windows 会自行派生 Bitmap，无需再次解码图片。
            if (format == StandardDataFormats.Bitmap && content.Contains(DibV5)) continue;
            snapshot.SetData(format, await content.GetDataAsync(format));
        }
        return snapshot;
    }

    public static async Task<string> ReadSelection(CancellationToken cancellation)
    {
        var original = await Snapshot();
        cancellation.ThrowIfCancellationRequested();
        var sequence = NativeMethods.GetClipboardSequenceNumber();
        NativeMethods.Chord(0x43);
        for (var i = 0; i < 40; i++)
        {
            await Task.Delay(20, cancellation);
            if (NativeMethods.GetClipboardSequenceNumber() == sequence) continue;
            var copiedSequence = NativeMethods.GetClipboardSequenceNumber();
            var content = Clipboard.GetContent();
            var text = content.Contains(StandardDataFormats.Text) ? await content.GetTextAsync() : "";
            if (NativeMethods.GetClipboardSequenceNumber() == copiedSequence) Clipboard.SetContent(original);
            return text;
        }
        return "";
    }

    public static async Task<string> Insert(string text, nint destination, CancellationToken cancellation)
    {
        var original = await Snapshot();
        cancellation.ThrowIfCancellationRequested();
        Copy(text);
        var sequence = NativeMethods.GetClipboardSequenceNumber();
        if (destination == 0) return "copiedToClipboard";
        if (NativeMethods.GetForegroundWindow() != destination && !NativeMethods.SetForegroundWindow(destination)) return "copiedToClipboard";
        await Task.Delay(80, cancellation);
        if (NativeMethods.GetForegroundWindow() != destination) return "copiedToClipboard";
        NativeMethods.Chord(0x56);
        await Task.Delay(300);
        if (NativeMethods.GetClipboardSequenceNumber() == sequence) Clipboard.SetContent(original);
        return "inserted";
    }
}
