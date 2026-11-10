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
        layout.ColumnDefinitions.Add(new() { Width = new(216) });
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
            var item = Controls.Accented(new ListViewItem { Tag = manifest["id"]!.GetValue<string>(), Content = Controls.Stack(4, name, description), Padding = new(10, 8, 10, 8), CornerRadius = new(8), Margin = new(0, 2, 0, 2) });
            list.Items.Add(item);
        }
        list.SelectionChanged += (_, _) => Run(async () =>
        {
            if (list.SelectedItem is not ListViewItem item) return;
            selectedExtension = (string)item.Tag;
            details.Content = await ExtensionDetails(extensions.First(e => e!["manifest"]!["id"]!.GetValue<string>() == selectedExtension)!);
        });
        var toolbar = Controls.Stack(8, Controls.Wrap(
            Controls.Button(L("安装扩展…", "Install…"), () => Run(ChoosePackage)),
            Controls.Button(L("重新加载", "Reload"), () => Run(() => core!.Call("reloadExtensions"))),
            Controls.Button(L("打开文件夹", "Open folder"), () => Run(async () => WindowsPlatform.Open((await core!.Call("extensionsDirectory"))!.GetValue<string>())))));
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
        var sections = new List<UIElement> { Controls.Stack(8, heading, Controls.OptionalText(manifest["description"]?.GetValue<string>() ?? "", opacity: 0.7), toggle) };
        if (ExtensionFlow(info) is { } flow) sections.Add(flow);
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
            sections.Add(Controls.Wrap(
                Controls.Button(L("导出…", "Export…"), () => Run(() => ExportPackage(id))),
                Controls.Button(L("卸载", "Uninstall"), () => Run(async () =>
                {
                    if (await Confirm(L("卸载扩展", "Uninstall extension"), L("删除扩展文件，保留扩展数据？", "Remove extension files and retain its data?"))) await core!.Call("uninstall", JsonValue.Create(id));
                }))));
        }
        return Task.FromResult<UIElement>(Controls.Page(sections.ToArray()));
    }

    /// 扩展的运行关系图：左边是订阅的 hook，右边是它运行后引出的 hook 及接着运行的扩展。
    /// 只画确实连着的关系，哪一边没有就不画哪一边，两边都没有时整张图都不画。
    private UIElement? ExtensionFlow(JsonNode info)
    {
        var sources = info["listens"]!.AsArray().Select(link => HookNode(link!)).ToList();
        var targets = info["emits"]!.AsArray().Select(link => HookNode(link!)).ToList();
        if (sources.Count == 0 && targets.Count == 0) return null;
        var hub = new Border { Width = 40, Height = 40, CornerRadius = new(10), Background = Controls.Brush("SaysoSelection"), Child = new FontIcon { Glyph = "\uEA86", FontSize = 20, Foreground = Controls.Brush("SaysoAccentText") } };
        var flow = FlowDiagram.Create(L("什么会触发它", "What triggers it"), sources, hub, L("它会触发什么", "What it triggers"), targets);
        flow.Opacity = info["enabled"]!.GetValue<bool>() ? 1 : 0.5;
        return flow;
    }

    /// hook 节点：hook 名、何时发生，以及另一端的扩展。
    private static UIElement HookNode(JsonNode link) =>
        FlowContent(link["event"]!.GetValue<string>(), link["description"]!.GetValue<string>(), link["relation"]?.GetValue<string>());

    private static StackPanel FlowContent(string label, string title, string? caption)
    {
        var content = Controls.Stack(Controls.Text(label, 11, 0.65), Controls.Text(title, 12));
        if (caption is not null) content.Children.Add(Controls.Text(caption, 11, 0.6));
        content.Spacing = 4;
        return content;
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

    private JsonNode? Manifest(string id) => extensions.FirstOrDefault(e => e!["manifest"]!["id"]!.GetValue<string>() == id)?["manifest"];
    private JsonNode? AppTableSpec(string id) => Manifest(id)?["tab"]?["appTable"];

    /// <summary>默认背景单独展示，每条补充背景由应用选择和文本组成。</summary>
    private async Task<UIElement> AppTablePage(string id, JsonNode spec)
    {
        var table = (await core!.Call("appTable", JsonValue.Create(id)))!;
        var apps = table["apps"]!.AsArray();
        var rows = table["rows"]!.AsArray();
        Task Save(string key, JsonNode? app, string text) =>
            core.Call("saveAppRow", new JsonObject { ["id"] = id, ["key"] = key, ["app"] = app?.DeepClone(), ["text"] = text });
        TextBox Field(string value, Action<string> changed)
        {
            var field = Controls.Field(value, changed, spec["placeholder"]?.GetValue<string>() ?? "");
            field.TextWrapping = TextWrapping.Wrap;
            field.MaxLength = spec["maxLength"]!.GetValue<int>();
            return field;
        }
        static Grid Line(UIElement leading, UIElement field)
        {
            var line = new Grid { ColumnSpacing = 14 };
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(196) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            Grid.SetColumn((FrameworkElement)field, 1);
            line.Children.Add(leading);
            line.Children.Add(field);
            return line;
        }
        UIElement AppLine(JsonNode row)
        {
            var key = row["key"]!.GetValue<string>();
            var app = row["app"];
            var text = row["text"]!.GetValue<string>();
            var chosen = app?["id"]?.GetValue<string>() ?? "";
            // 已被其他行选中的应用不可选。
            var taken = rows.Where(r => r!["key"]!.GetValue<string>() != key).Select(r => r!["app"]?["id"]?.GetValue<string>()).ToHashSet();
            var options = apps.Select(a => (a!["id"]!.GetValue<string>(), a["name"]?.GetValue<string>() ?? a["id"]!.GetValue<string>())).Prepend(("", L("选择应用", "Choose an app")));
            var choice = Controls.Choice(options, chosen, value =>
            {
                app = apps.FirstOrDefault(a => a!["id"]!.GetValue<string>() == value);
                Run(() => Save(key, app, text));
            }, value => !taken.Contains(value));
            choice.MinWidth = 0;
            choice.HorizontalAlignment = HorizontalAlignment.Stretch;
            var remove = Controls.IconButton("\uE738", L("删除这一行", "Remove this row"), () => Run(() => core.Call("deleteRecord", new JsonObject { ["id"] = id, ["key"] = key })));
            var appControls = new Grid { ColumnSpacing = 6, VerticalAlignment = VerticalAlignment.Center };
            appControls.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
            appControls.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
            Grid.SetColumn(remove, 1);
            appControls.Children.Add(choice);
            appControls.Children.Add(remove);
            return Line(appControls, Field(text, value => { text = value; Run(() => Save(key, app, text)); }));
        }
        var defaultField = Field(table["defaultText"]!.GetValue<string>(), value => Run(() => Save("default", null, value)));
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(defaultField, L("默认背景", "Default context"));
        var add = Controls.Button(L("添加应用", "Add an app"), () => Run(() => core.Call("addAppRow", JsonValue.Create(id))));
        var description = Manifest(id)?["description"]?.GetValue<string>() ?? "";
        var lines = rows.Select(row => AppLine(row!)).Append(add).ToArray();
        return Controls.Page(Controls.OptionalText(description, opacity: 0.68),
            Controls.Section(L("默认背景", "Default context"), defaultField),
            Controls.Section(L("按应用补充", "Per-app context"), lines));
    }

    private async Task<UIElement> ExtensionPage(string id, string query)
    {
        var rows = Controls.Stack(8);
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
        var toolbar = new Grid { ColumnSpacing = 12 };
        toolbar.ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
        toolbar.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        Grid.SetColumn(count, 1);
        toolbar.Children.Add(search);
        toolbar.Children.Add(count);
        return Controls.Page(toolbar, rows);
    }
}
