using System.ComponentModel;
using System.Diagnostics;

namespace Sayso.Platform;

internal static class RunningApplications
{
    public static IReadOnlySet<string> Names()
    {
        var pids = new HashSet<uint>();
        // 一次枚举可见、无所有者的顶层窗口，与 MainWindowHandle 的筛选规则一致。
        if (!NativeMethods.EnumWindows((window, _) =>
        {
            if (NativeMethods.IsWindowVisible(window) && NativeMethods.GetWindow(window, NativeMethods.WindowOwner) == 0
                && NativeMethods.GetWindowThreadProcessId(window, out var pid) != 0 && pid != Environment.ProcessId)
                pids.Add(pid);
            return true;
        }, 0)) throw new Win32Exception();

        // 从同一份进程快照取名称，不再为每个进程重复扫描桌面窗口。
        var names = new HashSet<string>(StringComparer.Ordinal);
        foreach (var process in Process.GetProcesses())
        {
            using (process)
            {
                if (pids.Contains((uint)process.Id)) names.Add(process.ProcessName);
            }
        }
        return names;
    }
}
