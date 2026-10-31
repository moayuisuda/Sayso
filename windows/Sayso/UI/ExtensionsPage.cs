using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Sayso.Platform;
using Windows.Storage.Pickers;

namespace Sayso.UI;

internal sealed partial class SettingsWindow
{
    private string? selectedExtension;

    private async Task<UIElement> ExtensionsPage()
    {
        var layout = new Grid();
        layout.ColumnDefinitions.Add(new() { Width = new(240) });
        layout.ColumnDefinitions.Add(new() { Width = new(1) });
        layout.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
        var sidebar = new Grid();
        sidebar.RowDefinitions.Add(new() { Height = new(1, GridUnitType.Star) });
        sidebar.RowDefinitions.Add(new() { Height = GridLength.Auto });
        var list = new ListView { SelectionMode = ListViewSelectionMode.Single, Padding = new(8), BorderThickness = new(0) };
        var details = new ContentControl { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch };
        foreach (var info in extensions)
        {
            var manifest = info!["manifest"]!;
            var name = Controls.Text(manifest["name"]!.GetValue<string>());
            name.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
            var description = Controls.Text((info["builtIn"]!.GetValue<bool>() ? L("内置", "Built-in") : L("外部扩展", "External")) + " · " + (info["enabled"]!.GetValue<bool>() ? L("已启用", "Enabled") : L("未启用", "Disabled")), 11, 0.6);
            var item = Controls.Accented(new ListViewItem { Tag = manifest["id"]!.GetValue<string>(), Content = Controls.Stack(name, description), Padding = new(10), CornerRadius = new(8), Margin = new(0, 2, 0, 2) });
            list.Items.Add(item);
        }
        list.SelectionChanged += (_, _) => Run(async () =>
        {
            if (list.SelectedItem is not ListViewItem item) return;
            selectedExtension = (string)item.Tag;
            details.Content = await ExtensionDetails(extensions.First(e => e!["manifest"]!["id"]!.GetValue<string>() == selectedExtension)!);
        });
        var toolbar = Controls.Stack(
            Controls.Horizontal(Controls.Button(L("安装扩展…", "Install…"), () => Run(ChoosePackage)), Controls.Button(L("重新加载", "Reload"), () => Run(() => core!.Call("reloadExtensions")))),
            Controls.Button(L("打开文件夹", "Open folder"), () => Run(async () => WindowsPlatform.Open((await core!.Call("extensionsDirectory"))!.GetValue<string>()))));
        toolbar.Padding = new(12);
        Grid.SetRow(toolbar, 1);
        sidebar.Children.Add(list); sidebar.Children.Add(toolbar);
        layout.Children.Add(sidebar);
        var line = new Border { Background = Controls.Brush("SaysoLine") }; Grid.SetColumn(line, 1); layout.Children.Add(line);
        Grid.SetColumn(details, 2); layout.Children.Add(details);
        list.SelectedItem = list.Items.Cast<ListViewItem>().FirstOrDefault(i => (string)i.Tag == selectedExtension) ?? list.Items.Cast<ListViewItem>().FirstOrDefault();
        var problems = (await core!.Call("extensionProblems"))!.AsArray();
        if (problems.Count > 0) toolbar.Children.Add(Controls.Text(string.Join("\n", problems.Select(p => p!["directory"]!.GetValue<string>() + ": " + p["reason"]!.GetValue<string>())), 11));
        return layout;
    }

    private Task<UIElement> ExtensionDetails(JsonNode info)
    {
        var manifest = info["manifest"]!;
        var id = manifest["id"]!.GetValue<string>();
        var toggle = Controls.Accented(new ToggleSwitch { IsOn = info["enabled"]!.GetValue<bool>(), OnContent = L("已启用", "Enabled"), OffContent = L("未启用", "Disabled") });
        toggle.Toggled += (_, _) => Run(async () =>
        {
            if (toggle.IsOn && !info["builtIn"]!.GetValue<bool>() && !await Confirm(L("启用扩展", "Enable extension"), info["permissions"]!.GetValue<string>()))
            {
                toggle.IsOn = false;
                return;
            }
            await core!.Call("setEnabled", new JsonObject { ["id"] = id, ["enabled"] = toggle.IsOn });
        });
        var heading = Controls.Text(manifest["name"]!.GetValue<string>(), 18);
        heading.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
        var sections = new List<UIElement> { Controls.Stack(heading, Controls.Text(manifest["description"]?.GetValue<string>() ?? "", 12, 0.7), toggle) };
        var commands = new List<UIElement>();
        foreach (var command in manifest["commands"]!.AsArray())
        {
            var commandId = command!["id"]!.GetValue<string>();
            var binding = Settings["commandBindings"]!.AsArray().FirstOrDefault(b => b!["command"]!["extensionId"]!.GetValue<string>() == id && b["command"]!["commandId"]!.GetValue<string>() == commandId);
            var route = binding?["route"];
            var current = route is null ? "none" : route["kind"]!.GetValue<string>() == "direct" ? "direct" : route["code"]!.GetValue<int>().ToString();
            var options = new List<(string, string)> { ("none", L("未分配", "Unassigned")), ("direct", L("直接", "Direct")) };
            options.AddRange(Enumerable.Range(65, 26).Select(code => (code.ToString(), HotkeyMonitor.KeyName((uint)code))));
            var choice = Controls.Choice(options, current, value => Run(async () => { await BindCommand(id, commandId, value); await Navigate("extensions"); }));
            var row = Controls.Stack(Controls.Text(command["title"]!.GetValue<string>(), 12), choice);
            if (route is not null)
            {
                var routeSettings = Settings["routes"]!.AsArray().First(r => JsonNode.DeepEquals(r!["key"], route))!;
                row.Children.Add(Controls.Choice([("speech", L("语音输入", "Speech")), ("selection", L("选中文本", "Selected text"))], routeSettings["input"]!.GetValue<string>(), value => Run(async () =>
                {
                    Settings = (await core!.Call("setRouteInput", new JsonObject { ["route"] = route.DeepClone(), ["input"] = value }))!;
                    Hotkeys!.Settings = Settings;
                })));
            }
            commands.Add(row);
        }
        if (commands.Count > 0) sections.Add(Controls.Section(L("命令与路由", "Commands and routes"), commands.ToArray()));
        sections.Add(Controls.Section(L("事件与能力", "Events and capabilities"), Controls.Text(info["permissions"]!.GetValue<string>(), 12, 0.75)));
        if (info["lastResult"] is { } result)
        {
            var outcome = result["status"]!.GetValue<string>() switch { "succeeded" => L("执行成功", "Succeeded"), "timedOut" => L("执行超时", "Timed out"), _ => result["message"]!.GetValue<string>() };
            var messages = new List<UIElement> { Controls.Text(outcome, 12) };
            foreach (var warning in result["warnings"]!.AsArray()) messages.Add(Controls.Text(warning!.GetValue<string>(), 11, 0.7));
            if (result["log"]!.GetValue<string>() is { Length: > 0 } log) messages.Add(Controls.Text(log, 11, 0.7));
            sections.Add(Controls.Section(L("最近执行", "Last run"), messages.ToArray()));
        }
        if (!info["builtIn"]!.GetValue<bool>())
        {
            sections.Add(Controls.Horizontal(
                Controls.Button(L("导出…", "Export…"), () => Run(() => ExportPackage(id))),
                Controls.Button(L("卸载", "Uninstall"), () => Run(async () =>
                {
                    if (await Confirm(L("卸载扩展", "Uninstall extension"), L("删除扩展文件，保留扩展数据？", "Remove extension files and retain its data?"))) await core!.Call("uninstall", JsonValue.Create(id));
                }))));
        }
        return Task.FromResult<UIElement>(Controls.Page(sections.ToArray()));
    }

    private async Task BindCommand(string extension, string command, string selectedRoute)
    {
        // UI 仅将选项转换为平台键码，绑定与路由修改由核心完成。
        JsonObject? route = selectedRoute switch
        {
            "none" => null,
            "direct" => new() { ["kind"] = "direct" },
            _ => new() { ["kind"] = "key", ["code"] = int.Parse(selectedRoute) }
        };
        Settings = (await core!.Call("bindCommand", new JsonObject
        {
            ["command"] = new JsonObject { ["extensionId"] = extension, ["commandId"] = command }, ["route"] = route
        }))!;
        Hotkeys!.Settings = Settings;
    }

    private async Task ChoosePackage()
    {
        var picker = new FileOpenPicker(); picker.FileTypeFilter.Add(".sayso");
        WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(this));
        if (await picker.PickSingleFileAsync() is { } file) await InstallPackage(file.Path);
    }

    internal async Task InstallPackage(string path)
    {
        var info = (await core!.Call("openPackage", JsonValue.Create(path)))!;
        var token = info["token"]!.GetValue<string>();
        try
        {
            if (await Confirm(info["manifest"]!["name"]!.GetValue<string>(), info["permissions"]!.GetValue<string>() + (info["replacing"]!.GetValue<bool>() ? L("\n将替换已安装版本。", "\nThis replaces the installed version.") : "")))
                await core.Call("installPackage", JsonValue.Create(token));
        }
        finally { await core.Call("discardPackage", JsonValue.Create(token)); }
    }

    private async Task ExportPackage(string id)
    {
        var picker = new FileSavePicker { SuggestedFileName = id };
        picker.FileTypeChoices.Add("Sayso extension", new List<string> { ".sayso" });
        WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(this));
        if (await picker.PickSaveFileAsync() is { } file) await core!.Call("exportPackage", new JsonObject { ["id"] = id, ["path"] = file.Path });
    }

    private async Task<UIElement> ExtensionPage(string id, string query)
    {
        var rows = Controls.Stack();
        var count = Controls.Text("", 11, 0.65);
        var search = new TextBox { PlaceholderText = L("搜索…", "Search…"), Text = query, FontSize = 13 };
        CancellationTokenSource? pending = null;
        async Task Load(string value)
        {
            var result = (await core!.Call("tabItems", new JsonObject { ["id"] = id, ["query"] = value }))!;
            rows.Children.Clear();
            count.Text = $"{result["total"]} " + L("条记录", "records");
            foreach (var item in result["items"]!.AsArray())
            {
                var key = item!["key"]!.GetValue<string>();
                rows.Children.Add(BlockRenderer.Render(item["block"]!, action => Run(async () =>
                {
                    if (action["type"]!.GetValue<string>() == "delete")
                    {
                        if (await Confirm(L("删除记录", "Delete record"), L("确认删除这条记录？", "Delete this record?"))) await core.Call("deleteRecord", new JsonObject { ["id"] = id, ["key"] = key });
                    }
                    else await core.Call("tabAction", new JsonObject { ["id"] = id, ["key"] = key, ["action"] = action.DeepClone() });
                }), L));
            }
            if (rows.Children.Count == 0) rows.Children.Add(Controls.Card(Controls.Text(L("暂无记录", "No records"), 16), Controls.Text(L("语音输入或划词翻译后，可在候选框中添加词条。", "Add terms from the candidate panel after speech or selection translation."), 12, 0.65)));
        }
        search.TextChanged += (_, _) => Run(async () =>
        {
            pending?.Cancel();
            var current = pending = new CancellationTokenSource();
            try { await Task.Delay(180, current.Token); await Load(search.Text); }
            catch (OperationCanceledException) { }
        });
        await Load(query);
        return Controls.Page(Controls.Stack(search, count, rows));
    }
}
