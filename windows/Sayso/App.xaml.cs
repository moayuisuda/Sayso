using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Sayso.Interop;
using Sayso.Platform;
using Sayso.UI;

namespace Sayso;

public partial class App : Application
{
    private SingleInstance? instance;
    private bool quitting;
    private CoreClient? core;
    private WindowsPlatform? platform;
    private HotkeyMonitor? hotkeys;
    private TrayIcon? tray;
    private SettingsWindow? settings;
    private CandidateWindow? candidate;
    internal static readonly string DataDirectory = Environment.GetCommandLineArgs().Contains("--smoke-test")
        ? Path.Combine(Path.GetTempPath(), "Sayso-smoke-" + Environment.ProcessId)
        : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Sayso");

    public App()
    {
        InitializeComponent();
        UnhandledException += (_, e) => { Log(e.Exception); };
    }

    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        var arguments = Environment.GetCommandLineArgs();
        var packagePath = arguments.Skip(1).FirstOrDefault(a => a.EndsWith(".sayso", StringComparison.OrdinalIgnoreCase));
        instance = new SingleInstance(arguments.Contains("--smoke-test"));
        if (!instance.IsFirst)
        {
            await instance.Notify(packagePath);
            instance.Dispose();
            Exit();
            return;
        }
        try
        {
            Directory.CreateDirectory(DataDirectory);
            platform = new WindowsPlatform(Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread(), arguments.Contains("--smoke-test") ? $"Sayso.Smoke.{Environment.ProcessId}" : "Sayso");
            core = await Task.Run(() => new CoreClient(DataDirectory, platform.Handle));
            platform.Core = core;
            settings = new SettingsWindow(core, platform);
            candidate = new CandidateWindow(core, settings.L, settings.Show, () => platform.AudioLevel);
            hotkeys = new HotkeyMonitor(core, action => settings.Run(action), candidate.Dismiss,
                arguments.Contains("--smoke-test") ? SmokeTests.InputTag : null);
            settings.Hotkeys = hotkeys;
            platform.Changed += OnChanged;
            await settings.Initialize();
            tray = new TrayIcon(WinRT.Interop.WindowNative.GetWindowHandle(settings), settings.Show, () => Quit(), settings.L);
            instance.Listen(Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread(), path =>
            {
                settings.Show();
                if (path is not null) settings.Run(() => settings.InstallPackage(path));
            });
            settings.Show();
            if (Environment.GetCommandLineArgs().Contains("--smoke-test")) await SmokeTests.Run(core, platform, settings, candidate, Quit);
            else if (packagePath is not null) await settings.InstallPackage(packagePath);
        }
        catch (Exception error)
        {
            Log(error);
            settings ??= new SettingsWindow(null, null);
            settings.Show();
            await settings.ShowError(error);
        }
    }

    private void OnChanged(string method, JsonNode? value)
    {
        if (settings is null || quitting) return;
        switch (method)
        {
            case "sessionChanged": candidate!.Update(value!); break;
            case "settingsChanged": settings.Run(settings.ReloadSettings); break;
            case "extensionsChanged": settings.Run(settings.ReloadExtensions); break;
            case "storeChanged": settings.RefreshStore(value!.GetValue<string>()); break;
        }
    }

    internal static void Log(Exception error) => Log(error.ToString());

    internal static void Log(string message)
    {
        Directory.CreateDirectory(DataDirectory);
        File.AppendAllText(Path.Combine(DataDirectory, "windows.log"), $"{DateTimeOffset.Now:O} {message}\n");
    }

    private async void Quit()
    {
        if (quitting) return;
        quitting = true;
        tray?.Dispose();
        hotkeys?.Dispose();
        if (core is not null) await core.Call("cancel");
        if (platform is not null) await Task.Run(platform.Stop);
        // 核心任务停止并释放回调后，再释放平台资源。
        if (core is not null) await Task.Run(core.Dispose);
        if (platform is not null) await Task.Run(platform.Dispose);
        instance?.Dispose();
        Exit();
    }
}
