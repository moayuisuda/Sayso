namespace Sayso.Platform;

internal sealed class TrayIcon : IDisposable
{
    private NativeMethods.NotifyIconData data;
    private readonly NativeMethods.SubclassProc callback;
    private readonly Action show, quit, download;
    private readonly Func<string, string, string> localize;
    /// 已发现的新版本号；没有时为 null。
    private readonly Func<string?> updateVersion;

    public TrayIcon(nint window, Action show, Action quit, Func<string, string, string> localize, Func<string?> updateVersion, Action download)
    {
        this.show = show;
        this.quit = quit;
        this.localize = localize;
        this.updateVersion = updateVersion;
        this.download = download;
        callback = WindowProc;
        data = new()
        {
            Size = (uint)System.Runtime.InteropServices.Marshal.SizeOf<NativeMethods.NotifyIconData>(),
            Window = window, Id = 1, Flags = 7, Callback = NativeMethods.TrayMessage,
            Icon = NativeMethods.LoadImage(0, Path.Combine(AppContext.BaseDirectory, "Assets", "Sayso.ico"), 1, 32, 32, 0x10),
            Tip = "Sayso", Info = "", InfoTitle = ""
        };
        NativeMethods.SetWindowSubclass(window, callback, 1, 0);
        if (!NativeMethods.Shell_NotifyIcon(0, ref data)) throw new InvalidOperationException("无法创建系统托盘图标。");
    }

    private nint WindowProc(nint window, uint message, nuint wparam, nint lparam, nuint id, nuint reference)
    {
        if (message == NativeMethods.TrayMessage)
        {
            if (lparam == 0x202 || lparam == 0x203) show();
            // NIN_BALLOONUSERCLICK：点击更新通知。
            if (lparam == 0x405) download();
            if (lparam == 0x205)
            {
                var menu = NativeMethods.CreatePopupMenu();
                try
                {
                    if (updateVersion() is { } version)
                    {
                        NativeMethods.AppendMenu(menu, 0, 3, localize($"新版本 {version} 可下载…", $"Download version {version}…"));
                        NativeMethods.AppendMenu(menu, 0x800, 0, "");
                    }
                    NativeMethods.AppendMenu(menu, 0, 1, localize("设置", "Settings"));
                    NativeMethods.AppendMenu(menu, 0, 2, localize("退出", "Quit"));
                    NativeMethods.GetCursorPos(out var point);
                    NativeMethods.SetForegroundWindow(window);
                    var command = NativeMethods.TrackPopupMenu(menu, 0x100 | 0x2, point.X, point.Y, 0, window, 0);
                    if (command == 1) show();
                    if (command == 2) quit();
                    if (command == 3) download();
                }
                finally { NativeMethods.DestroyMenu(menu); }
            }
            return 0;
        }
        return NativeMethods.DefSubclassProc(window, message, wparam, lparam);
    }

    /// 通过托盘图标显示系统通知（NIM_MODIFY + NIF_INFO）。
    public void NotifyUpdate(string title, string message)
    {
        var balloon = data;
        balloon.Flags = 0x10;
        balloon.InfoTitle = title;
        balloon.Info = message;
        balloon.InfoFlags = 1;
        NativeMethods.Shell_NotifyIcon(1, ref balloon);
    }

    public void Dispose()
    {
        NativeMethods.Shell_NotifyIcon(2, ref data);
        NativeMethods.RemoveWindowSubclass(data.Window, callback, 1);
        NativeMethods.DestroyIcon(data.Icon);
    }
}
