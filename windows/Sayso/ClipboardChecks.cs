using System.ComponentModel;
using System.Runtime.InteropServices;
using Sayso.Platform;
using Windows.ApplicationModel.DataTransfer;
using Windows.Storage.Streams;
using WinRT;

namespace Sayso;

/// <summary>以真实位域图片和 WinUI 输入框验证剪贴板备份、粘贴与原图恢复。</summary>
internal static class ClipboardChecks
{
    internal static async Task InsertOverBitmap()
    {
        var original = await ClipboardService.Snapshot();
        using var target = new InputTestWindow();
        var input = target.Input;
        try
        {
            // 32 位 BI_BITFIELDS 图片与截图工具的剪贴板格式一致，不能依赖 Bitmap 转码。
            var bitmap = new byte[140];
            using (var writer = new BinaryWriter(new MemoryStream(bitmap)))
            {
                writer.Write(124); writer.Write(2); writer.Write(2);
                writer.Write((ushort)1); writer.Write((ushort)32);
                writer.Write(3); writer.Write(16);
                writer.Seek(40, SeekOrigin.Begin);
                writer.Write(0x00FF0000u); writer.Write(0x0000FF00u);
                writer.Write(0x000000FFu); writer.Write(0xFF000000u);
                writer.Write(0x73524742u);
                writer.Seek(124, SeekOrigin.Begin);
                foreach (var pixel in new[] { 0xFFFF0000u, 0xFF00FF00u, 0xFF0000FFu, 0xFFFFFFFFu }) writer.Write(pixel);
            }
            var handle = target.Handle;
            if (!OpenClipboard(handle)) throw new Win32Exception();
            try
            {
                if (!EmptyClipboard()) throw new Win32Exception();
                var memory = GlobalAlloc(2, (nuint)bitmap.Length);
                if (memory == 0) throw new Win32Exception();
                try
                {
                    var address = GlobalLock(memory);
                    if (address == 0) throw new Win32Exception();
                    try { Marshal.Copy(bitmap, 0, address, bitmap.Length); }
                    finally { GlobalUnlock(memory); }
                    if (SetClipboardData(17, memory) == 0) throw new Win32Exception();
                    memory = 0; // 成功后内存归系统剪贴板所有。
                }
                finally { if (memory != 0) GlobalFree(memory); }
            }
            finally { CloseClipboard(); }
            if (!Clipboard.GetContent().Contains(StandardDataFormats.Bitmap)) throw new Exception("Expected the system-derived Bitmap format.");

            await target.Show();
            var delivery = await ClipboardService.Insert("Sayso 输入检查 괜찮아", target.Handle, CancellationToken.None);
            if (delivery != "inserted" || input.Text != "Sayso 输入检查 괜찮아") throw new Exception($"Paste failed: {delivery}, text={input.Text}");

            var restored = (await Clipboard.GetContent().GetDataAsync(ClipboardService.DibV5)).As<IRandomAccessStream>();
            using var reader = new DataReader(restored.GetInputStreamAt(0));
            await reader.LoadAsync((uint)restored.Size);
            var actual = new byte[restored.Size];
            reader.ReadBytes(actual);
            if (!actual.SequenceEqual(bitmap)) throw new Exception("Paste did not restore the original bitmap bytes.");
        }
        finally
        {
            Clipboard.SetContent(original);
        }
    }
    [DllImport("user32", SetLastError = true)] private static extern bool OpenClipboard(nint window);
    [DllImport("user32", SetLastError = true)] private static extern bool EmptyClipboard();
    [DllImport("user32")] private static extern bool CloseClipboard();
    [DllImport("user32", SetLastError = true)] private static extern nint SetClipboardData(uint format, nint memory);
    [DllImport("kernel32", SetLastError = true)] private static extern nint GlobalAlloc(uint flags, nuint size);
    [DllImport("kernel32", SetLastError = true)] private static extern nint GlobalLock(nint memory);
    [DllImport("kernel32")] private static extern bool GlobalUnlock(nint memory);
    [DllImport("kernel32")] private static extern nint GlobalFree(nint memory);
}
