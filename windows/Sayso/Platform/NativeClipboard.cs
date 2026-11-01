using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

namespace Sayso.Platform;

/// <summary>按 Win32 原始格式保存剪贴板，不经过 WinRT 的字符串、图片或文件列表转换。</summary>
internal static class NativeClipboard
{
    internal const uint UnicodeText = 13, DibV5 = 17;
    private static nint owner;
    private static readonly List<FormatData> privateData = [];
    private static readonly NativeMethods.SubclassProc ownerProcedure = OwnerProcedure;
    internal readonly record struct Version(uint Sequence, nint Owner);
    private static Version CurrentVersion => new(NativeMethods.GetClipboardSequenceNumber(), GetClipboardOwner());

    internal sealed class Snapshot : IDisposable
    {
        private readonly List<FormatData> formats = [];
        private bool disposed;

        internal static Snapshot Capture()
        {
            var snapshot = new Snapshot();
            try
            {
                uint format = 0;
                while (true)
                {
                    Marshal.SetLastPInvokeError(0);
                    format = EnumClipboardFormats(format);
                    if (format == 0)
                    {
                        if (Marshal.GetLastPInvokeError() != 0) throw new Win32Exception();
                        break;
                    }
                    // OWNERDISPLAY 只有窗口绘制协议，没有可转移的数据句柄。
                    if (format == 0x80) continue;
                    var source = GetClipboardData(format);
                    if (source == 0) throw new Win32Exception(Marshal.GetLastPInvokeError(), $"无法读取剪贴板格式 0x{format:X}。");
                    var copy = format is 14 or 0x8E ? CopyEnhMetaFileW(source, null)
                        : OleDuplicateData(source, (ushort)(format switch { 0x82 => 2u, 0x83 => 3u, _ => format }), 0);
                    if (copy == 0) throw new InvalidOperationException($"无法保存剪贴板格式 0x{format:X}。");
                    snapshot.formats.Add(new(format, copy));
                }
                return snapshot;
            }
            catch { snapshot.Dispose(); throw; }
        }

        internal async Task<bool> Restore(Version? expectedVersion = null)
        {
            ObjectDisposedException.ThrowIf(disposed, this);
            using var access = await Open(CancellationToken.None);
            // 比较与恢复在同一次剪贴板锁内完成，避免覆盖用户随后复制的内容。
            if (expectedVersion is { } version && CurrentVersion != version) return false;
            if (!EmptyClipboard()) throw new Win32Exception();
            foreach (var data in formats)
            {
                if (SetClipboardData(data.Format, data.Handle) == 0) throw new Win32Exception();
                if (data.Format is >= 0x200 and <= 0x3FF) privateData.Add(new(data.Format, data.Handle));
                data.Handle = 0; // 成功后由系统或剪贴板所有者窗口负责释放。
            }
            disposed = true;
            return true;
        }

        public void Dispose()
        {
            foreach (var data in formats) data.Dispose();
            formats.Clear();
            disposed = true;
        }
    }

    private sealed class FormatData(uint format, nint handle) : IDisposable
    {
        internal uint Format { get; } = format;
        internal nint Handle { get; set; } = handle;

        public void Dispose()
        {
            if (Handle == 0) return;
            switch (Format)
            {
                case 2 or 9 or 0x82: DeleteObject(Handle); break;
                case 14 or 0x8E: DeleteEnhMetaFile(Handle); break;
                case 3 or 0x83:
                    var pointer = GlobalLock(Handle);
                    if (pointer != 0)
                    {
                        DeleteMetaFile(Marshal.PtrToStructure<MetafilePicture>(pointer).Metafile);
                        GlobalUnlock(Handle);
                    }
                    GlobalFree(Handle);
                    break;
                default: GlobalFree(Handle); break;
            }
            Handle = 0;
        }
    }

    internal static async Task<Snapshot> Capture(CancellationToken cancellation = default)
    {
        using var access = await Open(cancellation);
        return Snapshot.Capture();
    }

    internal static async Task<Version> WriteText(string text, CancellationToken cancellation = default)
    {
        // 先分配完整数据，再修改剪贴板；文本必须以 UTF-16 NUL 结尾。
        var bytes = Encoding.Unicode.GetBytes(text + '\0');
        using var data = new FormatData(UnicodeText, GlobalAlloc(2, (nuint)bytes.Length));
        if (data.Handle == 0) throw new Win32Exception();
        var pointer = GlobalLock(data.Handle);
        if (pointer == 0) throw new Win32Exception();
        try { Marshal.Copy(bytes, 0, pointer, bytes.Length); }
        finally { GlobalUnlock(data.Handle); }
        using (await Open(cancellation))
        {
            if (!EmptyClipboard() || SetClipboardData(UnicodeText, data.Handle) == 0) throw new Win32Exception();
            data.Handle = 0;
        }
        // Windows 在 CloseClipboard 时完成格式发布并更新序列号，不能在关闭前取版本。
        // 若发布后已被其他程序复制覆盖，返回无效版本，后续不得粘贴或恢复旧快照。
        var version = CurrentVersion;
        return version.Owner == owner ? version : default;
    }

    internal static async Task<(string Text, Version Version)> ReadText(CancellationToken cancellation = default)
    {
        using var access = await Open(cancellation);
        if (!IsClipboardFormatAvailable(UnicodeText)) return ("", CurrentVersion);
        var data = GetClipboardData(UnicodeText);
        if (data == 0) throw new Win32Exception();
        var pointer = GlobalLock(data);
        if (pointer == 0) throw new Win32Exception();
        try { return (Marshal.PtrToStringUni(pointer) ?? "", CurrentVersion); }
        finally { GlobalUnlock(data); }
    }

    internal static async Task<bool> IsCurrent(Version version, CancellationToken cancellation)
    {
        using var access = await Open(cancellation);
        return CurrentVersion == version;
    }

    private sealed class Access : IDisposable
    {
        public void Dispose() => CloseClipboard();
    }

    private static async Task<Access> Open(CancellationToken cancellation)
    {
        if (owner == 0)
        {
            // 进程生命周期内保留一个 UI 线程的消息窗口。空所有者会导致 SetClipboardData 失败。
            owner = CreateWindowExW(0, "STATIC", "Sayso.Clipboard", 0, 0, 0, 0, 0, -3, 0, 0, 0);
            if (owner == 0 || !NativeMethods.SetWindowSubclass(owner, ownerProcedure, 1, 0)) throw new Win32Exception();
        }
        for (var attempt = 0; ; attempt++)
        {
            cancellation.ThrowIfCancellationRequested();
            if (OpenClipboard(owner)) return new Access();
            var error = Marshal.GetLastPInvokeError();
            // 系统剪贴板是跨进程互斥资源；只对短暂占用做有界等待。
            if (error != 5 || attempt == 19) throw new Win32Exception(error, "暂时无法访问系统剪贴板。");
            await Task.Delay(10, cancellation);
        }
    }

    private static nint OwnerProcedure(nint window, uint message, nuint wparam, nint lparam, nuint id, nuint data)
    {
        if (message == 0x0307) // WM_DESTROYCLIPBOARD：私有格式的句柄不会由 Windows 自动释放。
        {
            foreach (var item in privateData) item.Dispose();
            privateData.Clear();
        }
        return NativeMethods.DefSubclassProc(window, message, wparam, lparam);
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MetafilePicture { public int MappingMode, Width, Height; public nint Metafile; }
    [DllImport("user32", SetLastError = true)] private static extern bool OpenClipboard(nint window);
    [DllImport("user32")] private static extern bool CloseClipboard();
    [DllImport("user32", SetLastError = true)] private static extern bool EmptyClipboard();
    [DllImport("user32", SetLastError = true)] private static extern uint EnumClipboardFormats(uint format);
    [DllImport("user32", SetLastError = true)] private static extern nint GetClipboardData(uint format);
    [DllImport("user32", SetLastError = true)] private static extern nint SetClipboardData(uint format, nint memory);
    [DllImport("user32")] private static extern bool IsClipboardFormatAvailable(uint format);
    [DllImport("user32")] private static extern nint GetClipboardOwner();
    [DllImport("user32", CharSet = CharSet.Unicode, SetLastError = true)] private static extern nint CreateWindowExW(uint exStyle, string className, string name, uint style, int x, int y, int width, int height, nint parent, nint menu, nint instance, nint parameter);
    [DllImport("ole32")] private static extern nint OleDuplicateData(nint source, ushort format, uint flags);
    [DllImport("kernel32", SetLastError = true)] private static extern nint GlobalAlloc(uint flags, nuint size);
    [DllImport("kernel32", SetLastError = true)] private static extern nint GlobalLock(nint memory);
    [DllImport("kernel32")] private static extern bool GlobalUnlock(nint memory);
    [DllImport("kernel32")] private static extern nint GlobalFree(nint memory);
    [DllImport("gdi32")] private static extern bool DeleteObject(nint value);
    [DllImport("gdi32")] private static extern bool DeleteMetaFile(nint metafile);
    [DllImport("gdi32")] private static extern bool DeleteEnhMetaFile(nint metafile);
    [DllImport("gdi32", CharSet = CharSet.Unicode)] private static extern nint CopyEnhMetaFileW(nint metafile, string? filename);
}
