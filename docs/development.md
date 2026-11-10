# 开发说明

## 目录结构

Rust core 是设置、扩展、存储、识别、翻译与会话的唯一实现；macOS（Swift / SwiftUI）与 Windows（C# / WinUI 3）宿主只负责平台适配与界面。Windows 宿主的细节见 [windows.md](windows.md)，扩展协议见 [extension-protocol.md](extension-protocol.md)。

```
core/                     Rust core（macOS 与 Windows 共用）
├── sayso-core/src/
│   ├── protocol/           扩展协议：语言、块、操作、事件、清单与页签模板
│   ├── plugin/             JS 扩展运行时（QuickJS）、`sayso.*` API、扩展目录加载
│   ├── session/            会话状态机：输入快捷键 → 录音识别或读取选区 → 广播 input；候选框内容、停留时长、块操作
│   ├── asr/                识别来源注册表；火山引擎 / 千问 / Soniox（流式 WebSocket）、OpenAI 兼容 URL（本地或云端，批量）
│   ├── llm/                OpenAI 兼容 / Anthropic 客户端、服务预设、思考强度、连接预热
│   ├── extensions.rs       扩展启停、事件订阅和广播对象
│   ├── store.rs            扩展数据存储（串行、原子落盘）
│   ├── settings.rs         设置（settings.json）与密钥（config.json）
│   ├── package.rs          `.sayso` 扩展包
│   └── platform.rs         平台适配层接口：录音、选区、写入、朗读与状态推送
├── plugins/              内置扩展「混说翻译」「互译」「生词本」「应用背景」（JS，编译时嵌入）
├── sayso-shell/           跨平台 shell 执行进程，与宿主一起打包
└── sayso-windows-bridge/  Windows 的 C ABI：请求、状态回调与可取消的平台异步调用
Sources/
├── SaysoCore/            UniFFI 生成的 Swift 绑定（构建产物）
└── Sayso/                macOS 宿主：平台适配层与 SwiftUI 界面
    ├── Platform/           适配层实现（MacPlatform）
    ├── App/                应用组装与 core 状态镜像（CoreModel）
    ├── Audio/              麦克风 → 16kHz PCM
    ├── Input/              全局快捷键（CGEventTap）：单修饰键或组合键 → Sayso 输入入口
    ├── Injection/          剪贴板 + ⌘V 写入、读取选中文本、剪贴板快照
    ├── Speech/             系统 TTS 朗读
    └── UI/                 候选框、块组件、设置窗口、扩展管理与扩展页签
windows/Sayso/            Windows 原生宿主（C# / WinUI 3）
├── Interop/                Rust 核心实例、ABI 内存管理与串行设置事务
├── Platform/               WASAPI 录音、全局热键、剪贴板、SAPI 朗读、托盘与单实例
└── UI/                     与 macOS 一致的 Indigo 主题、顶部页签、紧凑卡片与 Acrylic 候选框
```

## 混说翻译

- LLM 生成完整目标语言句子的输出分段，每段只给出 `text` 及原文引用范围 `from` / `to`。原文由 `dictation/transcript.js` 保存为带编号的引用单元，模型不再重抄 `source`；例如 `I have a 苹果.` 的 5 个单元为 `I `、`have `、`a `、`苹果`、`.`，模型可将第 3–4 个单元译为 `an apple`。
- 模型负责整句语法及语序，输出分段可以移动原文片段、调整相邻冠词和主谓一致，而非在原位置逐词替换。引用必须覆盖原文词语且不能重复；独立标点可随整句重排删除或合并。高亮与词对由本地原文和实际输出的变化生成，不依赖模型的语言标签。共同前后文与纯大小写、空白、标点变化不作为生词。所有实际翻译或语法修正片段都提供加入生词本入口，不因句子长度静默丢弃。引用无效时保留识别原文并提示原因。
- 提示词由共享插件中的 `dictation/prompt.js` 统一组装。`dictation/languages.js` 只维护各语言自身的来源识别特点、目标语法要求及等义例句；仅当前语言对加载对应资料和可选的语言对规则。8 种语言的资料组合为全部 56 种有效语言对，无需复制模板。新增语言时补充资料，语言对矩阵测试会检查覆盖与规则隔离。
- 大模型根据语言配置识别「目标语言的发音被首轮 ASR 写成本地语言文字」的片段，结合整段读音线索和上下文还原目标语言，再处理真正的本地语言表达。正常本地表达、姓名与有歧义的片段不应仅因音近就被强行还原；纠错与翻译在同一次请求完成，不增加语音模型调用。
- 目标与本地语言书写系统不相交（如英/中）时，纯目标语言输入在本地即可判定，不调用 LLM、不增加延迟。
- 按下热键时即预先建立到翻译服务的连接；翻译短句时可在设置中关闭模型思考，进一步缩短延迟。

## 构建与测试

### macOS

```bash
cargo test --manifest-path core/Cargo.toml   # core 与内置扩展的测试
bash scripts/build-core.sh                   # 构建 core 并生成 Swift 绑定，之后可直接 swift build
INSTALL=1 bash scripts/bundle-app.sh         # 构建 core 与应用，打包并安装到 /Applications
```

打包脚本优先用登录钥匙串中的 Developer ID Application 证书签名，与发布版身份相同，本地构建与发布版之间切换不需要重新授权；没有时用 Apple Development 证书，都没有时退回 ad-hoc 签名，每次重新打包后需要重新授权。

### Windows

```powershell
./scripts/build-windows.ps1 -Test -Install
```

除 .NET 10 SDK、Rust MSVC 工具链、C++ Build Tools 与 Windows SDK 外，还需要 [Inno Setup 7](https://jrsoftware.org/isdl.php)（CI 固定使用 7.1.1，并校验 SHA-256）。脚本会从 PATH 或安装注册表定位编译器，也可通过 `-InstallerCompiler` 指定 `ISCC.exe` 路径。

脚本构建 Rust 动态库和 WinUI 3 客户端，再生成 `build/installers/Sayso-<版本>-windows-x64-Setup.exe`。`-Install` 打开安装向导，默认安装到 `%LOCALAPPDATA%\Programs\Sayso`，不需要管理员权限。`build/windows/` 为内部发布目录，自带 .NET 与 Windows App SDK 运行时。

`-Test` 执行 Rust 测试、真实 Windows 集成检查和安装包生命周期验证。安装包测试会在工作区的独立测试目录中安装、核对全部依赖、覆盖安装并卸载，验证快捷方式、文件关联和用户数据保留；需要在尚未安装 Sayso 的账户中运行。失败时保留测试目录内的安装日志，成功时清理测试目录。

修改打包规则后，无需重新编译客户端，可运行 `./scripts/package-windows.ps1`。安装、升级、卸载统一由 `scripts/windows-installer.iss` 定义，不再维护独立的 PowerShell 安装规则。

集成测试使用独立临时目录和独立凭据命名空间，报告写入 `build/windows/smoke-results.json`。这些检查覆盖本地能力，云端识别与翻译需要另外配置真实服务后验证。

## 发版

推送 `v` 开头的 tag 即触发 [release 流水线](../.github/workflows/release.yml)：

```bash
git tag v0.2.0
git push origin v0.2.0
```

流水线先跑 core 测试，再分别构建 `Sayso-<版本>-macOS-arm64.zip`（`Sayso.app`）与 `Sayso-<版本>-windows-x64-Setup.exe`，最后创建同名 GitHub Release 并附上两个安装文件。版本号取自 tag，写入 `Info.plist` 与 `Sayso.exe`；Windows 安装包从 `Sayso.exe` 读取版本，统一安装向导与「已安装的应用」中的版本信息。

应用启动后（之后每天一次）由 core 读取本仓库 GitHub Releases 的最新正式版，比当前版本新时发送系统通知，并在菜单栏 / 托盘菜单与设置「通用 › 关于」中显示下载入口。草稿与预发布版本不会触发提醒。

Windows 安装包包含全部运行时，创建开始菜单入口并注册 `.sayso` 打开方式。安装与卸载会通过应用的命名互斥量检查运行状态，提示先从托盘退出。稳定的 `AppId` 让新版覆盖同一安装，卸载仅移除安装文件及其注册项，保留 `%LOCALAPPDATA%\Sayso` 下的设置、密钥与扩展数据。

macOS 包只包含 Apple 芯片版本，用 Developer ID Application 证书签名（开启 Hardened Runtime，权限声明见 `Resources/Sayso.entitlements`）并经 Apple 公证，公证票据钉在 app 中。仓库需要配置以下 secrets：

| Secret | 内容 |
| --- | --- |
| `CSC_LINK` | Developer ID Application 证书导出的 `.p12`，base64 编码 |
| `CSC_KEY_PASSWORD` | `.p12` 的密码 |
| `APPLE_ID` | 提交公证的 Apple ID |
| `APPLE_APP_SPECIFIC_PASSWORD` | 该 Apple ID 的 App 专用密码 |
| `APPLE_TEAM_ID` | 开发者团队 ID |

## 数据位置

| | macOS | Windows |
| --- | --- | --- |
| 设置 | `~/Library/Application Support/Sayso/settings.json` | `%LOCALAPPDATA%\Sayso\settings.json` |
| 外部扩展 | `~/Library/Application Support/Sayso/Extensions/` | `%LOCALAPPDATA%\Sayso\Extensions\` |
| 扩展数据 | `~/Library/Application Support/Sayso/ExtensionData/<扩展 id>/` | `%LOCALAPPDATA%\Sayso\ExtensionData\<扩展 id>\` |
| API Key | `~/Library/Application Support/Sayso/config.json` | `%LOCALAPPDATA%\Sayso\config.json` |

生词本数据为 `ExtensionData/sayso.wordbook/words.json`。

## 图标

应用图标的唯一源文件是 [`Resources/AppIcon.svg`](../Resources/AppIcon.svg)，修改后运行以下命令重新生成 macOS `AppIcon.icns`、菜单栏模板 `MenuBarIcon.png` 与 Windows `Sayso.ico`：

```bash
swift scripts/render-icons.swift
```
