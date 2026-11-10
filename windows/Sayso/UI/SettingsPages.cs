using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Sayso.Platform;

namespace Sayso.UI;

internal sealed partial class SettingsWindow
{
    private IEnumerable<(string Value, string Title)> Options(JsonArray items) => items.Select(i => (i!["value"]!.GetValue<string>(), i["title"]!.GetValue<string>()));
    private ComboBox SettingChoice(string field, IEnumerable<(string Value, string Title)> choices, bool redraw = false, Func<string, bool>? enabled = null) =>
        Controls.Choice(choices, Settings[field]!.GetValue<string>(), value => Run(async () => { await Save(field, JsonValue.Create(value)); if (redraw) { BuildTabs(); await Navigate(selected); } }), enabled);

    /// 当前选用的识别来源；来源不可用时为 null。
    private JsonNode? SelectedAsrSource => asrSources.FirstOrDefault(s => s!["id"]!.GetValue<string>() == Settings["asrSource"]!.GetValue<string>());
    /// 来源不可用时不限制语言。
    private static bool Recognizes(JsonNode? source, string language) => source is null || source["languages"]!.AsArray().Any(l => l!.GetValue<string>() == language);
    /// 来源能否识别当前的目标语言与本地语言。
    private bool SupportsLanguages(JsonNode source) => Recognizes(source, Settings["targetLanguage"]!.GetValue<string>()) && Recognizes(source, Settings["nativeLanguage"]!.GetValue<string>());

    /// 识别来源只支持部分语言时，提示可选范围。
    private string LanguageCaption()
    {
        var caption = L("正在学习的语言", "The language you are learning");
        if (SelectedAsrSource is not { } source || source["languages"]!.AsArray().Count >= languages.Count) return caption;
        var names = string.Join(L("、", ", "), languages.Where(l => Recognizes(source, l!["value"]!.GetValue<string>())).Select(l => l!["title"]!.GetValue<string>()));
        return caption + L($"。语音模型「{source["title"]!.GetValue<string>()}」只支持：{names}", $". Speech model \"{source["title"]!.GetValue<string>()}\" only supports: {names}");
    }
    private TextBox SettingField(string field) => Controls.Field(Settings[field]!.GetValue<string>(), value => Run(() => Save(field, JsonValue.Create(value))));

    private UIElement GeneralPage()
    {
        var autoSpeak = Controls.Accented(new CheckBox { Content = L("生成翻译块时自动朗读", "Read translated blocks aloud"), IsChecked = Settings["autoSpeak"]!.GetValue<bool>(), FontSize = 13 });
        autoSpeak.Click += (_, _) => Run(() => Save("autoSpeak", JsonValue.Create(autoSpeak.IsChecked == true)));
        var rate = Controls.Accented(new Slider { Minimum = 0.3, Maximum = 0.6, Value = Settings["speechRate"]!.GetValue<double>(), Width = 180, StepFrequency = 0.01 });
        rate.ValueChanged += (_, _) => Run(() => Save("speechRate", JsonValue.Create(rate.Value)));
        return Controls.Page(
            Controls.Section(L("语言", "Languages"),
                Controls.Row(L("目标语言", "Target language"), SettingChoice("targetLanguage", Options(languages), enabled: value => Recognizes(SelectedAsrSource, value)), LanguageCaption()),
                Controls.Row(L("本地语言", "Native language"), SettingChoice("nativeLanguage", Options(languages), true, value => Recognizes(SelectedAsrSource, value)), L("不会说的词直接用本地语言说，会自动翻译成目标语言。", "Say unfamiliar words in your native language to translate them in place."))),
            Controls.Section(L("快捷键", "Shortcuts"),
                ShortcutRow("speech"), ShortcutRow("selection"),
                Controls.Row(L("语音触发方式", "Voice shortcut mode"), SettingChoice("hotkeyStyle",
                    [("hold", L("按住说话", "Hold to talk")), ("toggle", L("按一下开始 / 结束", "Press to toggle"))]))),
            Controls.Section(L("朗读", "Pronunciation"),
                Controls.Row(L("自动朗读", "Read aloud"), autoSpeak),
                Controls.Row(L("系统语音", "System voices"), Controls.Button(L("管理朗读语音", "Manage voices"), () => WindowsPlatform.Open("ms-settings:speech")),
                    L("朗读需要安装对应语言的系统语音，可在 Windows 语音设置中添加。安装完成后重启 Sayso。", "Install a system voice for each language in Windows Speech settings, then restart Sayso.")),
                Controls.Row(L("语速", "Speed"), Controls.Wrap(rate, Controls.Button(L("试听", "Preview"), () => Run(() => Task.Run(() => platform!.Speak("Practice makes perfect.", "en-US", (float)rate.Value))))))),
            Controls.Section(L("权限", "Permissions"),
                Controls.Row(L("麦克风", "Microphone"), Controls.Button(L("打开麦克风设置", "Microphone settings"), () => WindowsPlatform.Open("ms-settings:privacy-microphone")), L("请允许桌面应用访问麦克风。录音只在快捷键触发时进行。", "Allow desktop apps to access the microphone. Recording starts only when triggered.")),
                Controls.Row(L("数据文件夹", "Data folder"), Controls.Button(L("打开文件夹", "Open folder"), () => WindowsPlatform.Open(App.DataDirectory)), L("API Key 保存在数据文件夹的 config.json 中。", "API keys are stored in config.json in the data folder."))),
            Controls.Section(L("关于", "About"), VersionRow())
        );
    }

    private string InputTitle(string input) => input == "speech" ? L("语音输入", "Voice input") : L("划词", "Selected text");

    private UIElement ShortcutRow(string input)
    {
        var key = Settings["shortcuts"]!.AsArray().FirstOrDefault(item => item!["input"]!.GetValue<string>() == input)?["key"];
        var recorder = Controls.Button(key is null ? L("设置快捷键", "Set shortcut") :
            HotkeyMonitor.KeyName(key["keyCode"]!.GetValue<uint>(), key["modifiers"]!.GetValue<ulong>()), () => { });
        recorder.Click += (_, _) =>
        {
            recorder.Content = L("请按下快捷键…", "Press a shortcut…");
            Hotkeys!.KeyRecorded = (code, modifiers) => Run(() => SetShortcut(input, new JsonObject { ["keyCode"] = code, ["modifiers"] = modifiers }));
            Hotkeys.IsRecordingKey = true;
        };
        recorder.MinWidth = 130;
        var field = Controls.Wrap(recorder);
        if (key is not null) field.Children.Add(Controls.IconButton("\uE711", L("清除", "Clear"), () => Run(() => SetShortcut(input, null))));
        return Controls.Row(InputTitle(input), field);
    }

    private async Task SetShortcut(string input, JsonObject? key)
    {
        Settings = (await core!.Call("setShortcut", new JsonObject { ["input"] = input, ["key"] = key }))!;
        Hotkeys!.Settings = Settings;
        await Navigate("general");
    }

    /// 当前版本与手动检查更新；已发现新版本时显示下载入口。
    private UIElement VersionRow()
    {
        var status = Controls.Text("", 12, 0.62);
        var download = Controls.Button(L("前往下载", "Download"), OpenUpdate);
        var feedback = Controls.Wrap(status, download);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(feedback, "UpdateFeedback");
        void SetStatus(string message)
        {
            status.Text = message;
            // 隐藏整行，避免状态文本为空时仍留下行高和间距。
            feedback.Visibility = string.IsNullOrWhiteSpace(message) ? Visibility.Collapsed : Visibility.Visible;
        }
        void ShowUpdate()
        {
            download.Visibility = Update is null ? Visibility.Collapsed : Visibility.Visible;
            if (Update is { } update) SetStatus(L($"新版本 {update["version"]!.GetValue<string>()} 已发布", $"Version {update["version"]!.GetValue<string>()} is available"));
        }
        var check = Controls.Button(L("检查更新", "Check for updates"), () => { });
        check.Click += (_, _) => Run(async () =>
        {
            check.IsEnabled = false;
            SetStatus(L("正在检查…", "Checking…"));
            try
            {
                Update = await core!.Call("checkForUpdate");
                if (Update is null) SetStatus(L("已是最新版本", "You're up to date"));
                ShowUpdate();
            }
            catch (Exception error) { SetStatus(error.Message); }
            finally { check.IsEnabled = true; }
        });
        SetStatus("");
        ShowUpdate();
        return Controls.Row(L("版本", "Version"), Controls.Stack(6, Controls.Wrap(Controls.Text(appVersion), check), feedback));
    }

    private async Task<UIElement> ModelsPage() => Controls.Page(await SpeechModelSection(), await LlmSection());

    private async Task<UIElement> SpeechModelSection()
    {
        var sourceId = Settings["asrSource"]!.GetValue<string>();
        var source = SelectedAsrSource;
        var rows = new List<UIElement>
        {
            Controls.Row(L("服务", "Service"), Controls.Choice(asrSources.Select(s => (s!["id"]!.GetValue<string>(), s["title"]!.GetValue<string>())), sourceId,
                value => Run(async () => { await Save("asrSource", JsonValue.Create(value)); await Navigate("models"); }),
                // 不能识别当前语言的来源不可选，需先在通用页改语言。
                value => SupportsLanguages(asrSources.First(s => s!["id"]!.GetValue<string>() == value)!)), source?["caption"]?.GetValue<string>())
        };
        if (source is not null)
        {
            foreach (var field in source["fields"]!.AsArray())
            {
                var fieldKey = field!["key"]!.GetValue<string>();
                var value = (await core!.Call("asrValue", new JsonObject { ["source"] = sourceId, ["field"] = fieldKey }))!.GetValue<string>();
                void Set(string changed) => Run(() => core.Call("setAsrValue", new JsonObject { ["source"] = sourceId, ["field"] = fieldKey, ["value"] = changed }));
                FrameworkElement control = field["kind"]!.GetValue<string>() switch
                {
                    "secret" => Controls.Secret(value, Set),
                    "choice" => Controls.Choice(Options(field["options"]!.AsArray()), value, Set),
                    _ => Controls.Field(value, Set)
                };
                rows.Add(Controls.Row(field["title"]!.GetValue<string>(), control, field["caption"]?.GetValue<string>()));
            }
            if (source["link"] is { } link) rows.Add(Controls.Row("", Controls.Button(link["title"]!.GetValue<string>(), () => WindowsPlatform.Open(link["url"]!.GetValue<string>()))));
        }
        return Controls.Section(L("语音模型", "Speech model"), rows.ToArray());
    }

    private async Task<UIElement> LlmSection()
    {
        var provider = Settings["llmProvider"]!.GetValue<string>();
        var rows = new List<UIElement>
        {
            Controls.Row(L("服务", "Provider"), SettingChoice("llmProvider", Options(providers), true))
        };
        if (providers.First(p => p!["value"]!.GetValue<string>() == provider)!["requiresKey"]!.GetValue<bool>())
        {
            var secret = (await core!.Call("secret", JsonValue.Create(provider)))!.GetValue<string>();
            rows.Add(Controls.Row("API Key", Controls.Secret(secret, value => Run(() => core.Call("setSecret", new JsonObject { ["provider"] = provider, ["value"] = value })))));
        }
        rows.Add(Controls.Row("Base URL", SettingField("llmBaseUrl")));
        rows.Add(Controls.Row(L("模型", "Model"), SettingField("llmModel")));
        rows.Add(Controls.Row(L("思考强度", "Reasoning"), SettingChoice("llmReasoning", [("modelDefault", L("模型默认", "Model default")), ("none", L("关闭（最快）", "Off (fastest)")), ("low", L("低", "Low")), ("medium", L("中", "Medium")), ("high", L("高", "High"))]),
            L("仅混说时调用模型。翻译短句可关闭思考；若服务不支持该参数，请选择模型默认。", "Only mixed language input calls the model. Turn reasoning off for short translations, or use Model default if unsupported.")));
        var output = Controls.OptionalText();
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(output, "TranslationTestFeedback");
        var test = Controls.Button(L("测试翻译", "Run test"), () => { });
        test.Background = Controls.Accent;
        test.Foreground = new SolidColorBrush(Microsoft.UI.Colors.White);
        test.Click += (_, _) => Run(async () =>
        {
            test.IsEnabled = false;
            output.Text = L("正在翻译…", "Translating…");
            try { output.Text = (await core!.Call("testLlm"))!.GetValue<string>(); }
            catch (Exception error) { output.Text = error.Message; }
            finally { test.IsEnabled = true; }
        });
        rows.Add(Controls.Row(L("连通性", "Connection"), Controls.Stack(6, test, output), L("用当前模型翻译一句示例，检查地址、密钥与模型名是否正确。", "Translate a sample to verify the URL, key and model name.")));
        return Controls.Section(L("大模型", "Large language model"), rows.ToArray());
    }
}
