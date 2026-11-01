using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using Sayso.Platform;

namespace Sayso;

/// <summary>以真实位域图片和 WinUI 输入框验证剪贴板备份、粘贴与原图恢复。</summary>
internal static class ClipboardChecks
{
    internal static async Task MixedFormatsAndConcurrentCopy()
    {
        using var original = await ClipboardService.Snapshot();
        using var target = new InputTestWindow();
        var binary = RegisterClipboardFormatW("Sayso.Integration.Binary");
        var dropEffect = RegisterClipboardFormatW("Preferred DropEffect");
        if (binary == 0 || dropEffect == 0) throw new Win32Exception();
        var fileList = new byte[20 + Encoding.Unicode.GetByteCount(Environment.ProcessPath + "\0\0")];
        BitConverter.GetBytes(20).CopyTo(fileList, 0);
        BitConverter.GetBytes(1).CopyTo(fileList, 16);
        Encoding.Unicode.GetBytes(Environment.ProcessPath + "\0\0").CopyTo(fileList, 20);
        try
        {
            await OpenForCheck(target.Handle);
            try
            {
                if (!EmptyClipboard()) throw new Win32Exception();
                SetBytes(13, Encoding.Unicode.GetBytes("原始文字 괜찮아\0"));
                SetBytes(15, fileList);
                SetBytes(binary, [0, 255, 128, 0, 17, 254, 1]);
                SetBytes(dropEffect, BitConverter.GetBytes(1u));
            }
            finally { CloseClipboard(); }
            var expected = new Dictionary<uint, byte[]>();
            foreach (var format in new[] { 13u, 15u, binary, dropEffect }) expected[format] = await ReadBytes(target.Handle, format);
            using (var saved = await ClipboardService.Snapshot())
            {
                var sequence = await ClipboardService.Copy("临时粘贴内容");
                if (!await saved.Restore(sequence)) throw new Exception("An owned clipboard snapshot was not restored.");
            }
            foreach (var entry in expected)
                if (!(await ReadBytes(target.Handle, entry.Key)).SequenceEqual(entry.Value)) throw new Exception($"Clipboard format 0x{entry.Key:X} changed during restoration.");

            using var previous = await ClipboardService.Snapshot();
            var ownedSequence = await ClipboardService.Copy("本次写入");
            await ClipboardService.Copy("用户随后复制的内容");
            if (await previous.Restore(ownedSequence) || (await NativeClipboard.ReadText()).Text != "用户随后复制的内容")
                throw new Exception("Restoring a stale snapshot overwrote a newer clipboard copy.");
        }
        finally { await original.Restore(); }
    }

    internal static async Task InsertOverBitmap()
    {
        using var original = await ClipboardService.Snapshot();
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
            await OpenForCheck(handle);
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
            if (!IsClipboardFormatAvailable(2)) throw new Exception("Expected the system-derived Bitmap format.");

            await target.Show();
            var delivery = await ClipboardService.Insert("Sayso 输入检查 괜찮아", target.Handle, CancellationToken.None);
            if (delivery != "inserted" || input.Text != "Sayso 输入检查 괜찮아") throw new Exception($"Paste failed: {delivery}, text={input.Text}");

            var actual = await ReadBytes(handle, 17);
            if (!actual.SequenceEqual(bitmap)) throw new Exception("Paste did not restore the original bitmap bytes.");
        }
        finally
        {
            await original.Restore();
        }
    }
    [DllImport("user32", SetLastError = true)] private static extern bool OpenClipboard(nint window);
    [DllImport("user32", CharSet = CharSet.Unicode, SetLastError = true)] private static extern uint RegisterClipboardFormatW(string name);
    [DllImport("user32")] private static extern bool IsClipboardFormatAvailable(uint format);
    [DllImport("user32", SetLastError = true)] private static extern nint GetClipboardData(uint format);
    [DllImport("kernel32")] private static extern nuint GlobalSize(nint memory);
    [DllImport("user32", SetLastError = true)] private static extern bool EmptyClipboard();
    [DllImport("user32")] private static extern bool CloseClipboard();
    [DllImport("user32", SetLastError = true)] private static extern nint SetClipboardData(uint format, nint memory);
    [DllImport("kernel32", SetLastError = true)] private static extern nint GlobalAlloc(uint flags, nuint size);
    [DllImport("kernel32", SetLastError = true)] private static extern nint GlobalLock(nint memory);
    [DllImport("kernel32")] private static extern bool GlobalUnlock(nint memory);
    [DllImport("kernel32")] private static extern nint GlobalFree(nint memory);

    private static void SetBytes(uint format, byte[] bytes)
    {
        var memory = GlobalAlloc(0x42, (nuint)bytes.Length);
        if (memory == 0) throw new Win32Exception();
        try
        {
            var pointer = GlobalLock(memory);
            if (pointer == 0) throw new Win32Exception();
            try { Marshal.Copy(bytes, 0, pointer, bytes.Length); }
            finally { GlobalUnlock(memory); }
            if (SetClipboardData(format, memory) == 0) throw new Win32Exception();
            memory = 0;
        }
        finally { if (memory != 0) GlobalFree(memory); }
    }

    private static async Task<byte[]> ReadBytes(nint owner, uint format)
    {
        await OpenForCheck(owner);
        try
        {
            var memory = GetClipboardData(format);
            if (memory == 0) throw new Win32Exception();
            var pointer = GlobalLock(memory);
            if (pointer == 0) throw new Win32Exception();
            try
            {
                var bytes = new byte[checked((int)GlobalSize(memory))];
                Marshal.Copy(pointer, bytes, 0, bytes.Length);
                return bytes;
            }
            finally { GlobalUnlock(memory); }
        }
        finally { CloseClipboard(); }
    }

    private static async Task OpenForCheck(nint owner)
    {
        // 验证端独立使用 Win32 读写；系统查看器也可能短暂持有这把跨进程锁。
        for (var attempt = 0; ; attempt++)
        {
            if (OpenClipboard(owner)) return;
            var error = Marshal.GetLastPInvokeError();
            if (error != 5 || attempt == 19) throw new Win32Exception(error);
            await Task.Delay(10);
        }
    }
}
