# 言出 Sayso

面向外语学习者的跨平台语音输入工具。用正在学的语言说话即可输入；遇到不会说的词，直接用母语说，
它会被翻译成目标语言、以高亮块显示在候选框里并朗读一遍，同时记入生词本。

## 使用流程

1. 在「设置 → 通用」选择**目标语言**（默认英语）与**本地语言**（默认中文）。
2. **语音翻译**（内置扩展）：按住入口键（默认**右 Option**）说话，松开结束；也可改为「按一下开始/结束」。录音中按 Esc 取消。
   - 正常说英语 → 原样输入。
   - 英语中夹杂中文，例如「I have a **苹果**」：翻译为贴合上下文的英文，输入 `I have an apple.`；
     候选框中译文以**荧光高亮**显示，下方列出「译文 · 原文」词条，点击可重听；每个翻译块生成时**自动朗读一次**。
3. **划词翻译**（内置扩展）：在任意应用中选中文字，按住**右 Option** 按 **T**（路由键在「设置 → 扩展」中修改或清除）。选中英文译为中文，
   选中中文译为英文，结果显示在同一个候选框里，英文一侧自动朗读。
4. **生词本**（内置扩展）：语音输入翻译出的词句、划词查到的单词和短语会逐个列在候选框里，每个词可单独「加入生词本」，
   也可点「全部加入生词本」一次加入。
   已加入的词在「设置 → 生词本」页签中可搜索、朗读、删除。
5. 候选框右上角的 × 可随时关闭；内容较长时可滚动，录音中的实时文字自动跟随最新内容；鼠标悬停时不会自动隐藏。

本地语音模型请自行启动独立服务，然后在「模型配置 → 语音模型」选择「OpenAI 兼容 / 本地服务」：
填写 Base URL（例如 `http://127.0.0.1:8000/v1`）与服务实际使用的模型名称；服务无需认证时 API Key 留空。
服务需支持 `POST /audio/transcriptions`，Sayso 在录音结束后上传整段 WAV 获取转写结果。

## 扩展系统

Sayso 是语言输入的中枢：把你说的话、选中的文本变成标准输入，交给扩展再加工。内置的语音翻译、划词翻译、生词本本身就是扩展：

- **命令与路由**：扩展提供的动作（如「翻译并输入」「添加待办」）。所有命令都从**统一入口键**（默认右 Option）触发，
  每个命令在「设置 → 扩展」中设一个**路由**：「直接」即只按入口键，或一个**路由键**——按住入口键期间按一下。
  **输入来源由路由决定**：语音（说一句话）或选区（选中的文本），扩展拿到的都是同一种文本 `input.text`，同一个命令换个路由就能处理另一种输入。
  **同一路由可绑定多个命令**，同时触发：例如待办也设为「直接」，说一句话既翻译写入、也记成待办；设为路由键 D，则按住右 Option 说话、松开前按 D 就只记待办。
- **事件**：命令触发（`command`）、用户点击了扩展的按钮（`action`）、某个扩展翻译出了词对（`translation`）、文字已写入前台应用（`inserted`）。
  事件中附带候选框里已有的块，扩展可以读取并加以利用；扩展也可以广播自己的翻译结果给其他扩展。
- **块**：扩展用固定组件库（`text` / `term` / `card` / `status`）往候选框添加内容，与内置内容同一套渲染。
- **操作**：块上的朗读、复制、打开链接、写入输入框、回调扩展、删除记录。
- **数据与页签**：每个扩展拥有由 Sayso 托管的数据集合，并可声明一个独立页签，按模板渲染自己的数据。
- **能力**：扩展声明需要的能力（`blocks` / `store` / `insert` / `llm` / `publish` / `fs`），外部扩展启用时由用户确认。
  `llm` 让扩展直接用你配置的翻译模型，不需要自带 API Key；内置的语音翻译、划词翻译也只用这些能力，外部扩展同样能实现。

扩展是一个 JS 模块（`main.js`），按事件类型导出处理函数，通过全局的 `sayso.*` API 使用宿主能力；由 Sayso 内嵌的 QuickJS 运行，
不依赖平台，macOS 与 Windows 共用同一份扩展。协议与数据结构见 **[docs/extension-protocol.md](docs/extension-protocol.md)**。

**分享扩展**：在「设置 → 扩展」点「导出…」，扩展文件夹原样打包成一个 `.sayso` 文件（包含代码与资源，不含你的数据）；
对方双击即可安装，不需要终端，也不会再下载任何东西。

示例扩展复制到 `~/Library/Application Support/Sayso/Extensions/` 后在「设置 → 扩展」中重新加载并启用即可试用：

- [`examples/extensions/favorites`](examples/extensions/favorites)：语音翻译、划词翻译出单词时显示「收藏」按钮，收藏的词有自己的页签。
- [`examples/extensions/explain`](examples/extensions/explain)：讲解一句外语的语法，用 `sayso.llm` 调用你配置的模型；路由设为选区即讲解选中的句子。
- [`examples/extensions/todo`](examples/extensions/todo)：命令「添加待办」，把说的话或选中的文字记为待办，显示在「待办」页签，点「完成」删除；
  在「设置 → 扩展」中为它设路由键（如 D）并选择输入来源，或设为「直接」与语音翻译同时触发。
- [`examples/extensions/obsidian`](examples/extensions/obsidian)：把 Sayso 每次写入的文字追加到 Obsidian 当天的笔记，
  启用前在 `config.json` 中填写库目录。

## 架构

```
core/                     Rust core（macOS 与 Windows 共用）
├── sayso-core/src/
│   ├── protocol/           扩展协议：语言、块、操作、事件、清单与页签模板
│   ├── plugin/             JS 扩展运行时（QuickJS）、`sayso.*` API、扩展目录加载
│   ├── session/            会话状态机：路由 → 录音识别或读取选区 → 命令；候选框内容、停留时长、块操作
│   ├── asr/                识别来源注册表；火山引擎 / 千问 / Soniox（流式 WebSocket）、OpenAI 兼容 URL（本地或云端，批量）
│   ├── llm/                OpenAI 兼容 / Anthropic 客户端、服务预设、思考强度、连接预热
│   ├── extensions.rs       扩展启停、命令路由、广播对象
│   ├── store.rs            扩展数据存储（串行、原子落盘）
│   ├── settings.rs         设置（settings.json；密钥经平台安全存储）
│   ├── package.rs          `.sayso` 扩展包
│   └── platform.rs         平台适配层接口：录音、选区、写入、朗读、安全存储与状态推送
├── plugins/              内置扩展「语音翻译」「划词翻译」「生词本」（JS，编译时嵌入）
└── sayso-windows-bridge/  Windows 的 C ABI：请求、状态回调与可取消的平台异步调用
Sources/
├── SaysoCore/            UniFFI 生成的 Swift 绑定（构建产物）
└── Sayso/                macOS 宿主：平台适配层与 SwiftUI 界面
    ├── Platform/           适配层实现（MacPlatform）
    ├── App/                应用组装与 core 状态镜像（CoreModel）
    ├── Audio/              麦克风 → 16kHz PCM
    ├── Input/              全局热键（CGEventTap）：统一入口键 + 路由键 → 路由
    ├── Injection/          剪贴板 + ⌘V 写入、读取选中文本、剪贴板快照
    ├── Speech/             系统 TTS 朗读
    └── UI/                 候选框、块组件、设置窗口、扩展管理与扩展页签
windows/Sayso/            Windows 原生宿主（C# / WinUI 3）
├── Interop/                Rust 核心实例、ABI 内存管理与串行设置事务
├── Platform/               WASAPI 录音、全局热键、剪贴板、SAPI 朗读、凭据管理器、托盘与单实例
└── UI/                     与 macOS 一致的 Indigo 主题、顶部页签、紧凑卡片与 Acrylic 候选框
```

**混说翻译的设计要点**

- LLM 生成完整目标语言句子的输出分段，每段只给出 `text` 及原文引用范围 `from` / `to`。
  原文由 `dictation/transcript.js` 保存为带编号的引用单元，模型不再重抄 `source`；例如 `I have a 苹果.` 的 5 个单元为 `I `、`have `、`a `、`苹果`、`.`，模型可将第 3–4 个单元译为 `an apple`。
- 模型负责整句语法及语序，输出分段可以移动原文片段、调整相邻冠词和主谓一致，而非在原位置逐词替换。
  引用必须覆盖原文词语且不能重复；独立标点可随整句重排删除或合并。高亮与词对由本地原文和实际输出的变化生成，不依赖模型的语言标签。共同前后文与纯大小写、空白、标点变化不作为生词。
  所有实际翻译或语法修正片段都提供加入生词本入口，不因句子长度静默丢弃。引用无效时保留识别原文并提示原因。
- 提示词由共享插件中的 `dictation/prompt.js` 统一组装。`dictation/languages.js` 只维护各语言自身的来源识别特点、目标语法要求及等义例句；仅当前语言对加载对应资料和可选的语言对规则。8 种语言的资料组合为全部 56 种有效语言对，无需复制模板。新增语言时补充资料，语言对矩阵测试会检查覆盖与规则隔离。
- 大模型根据语言配置识别「目标语言的发音被首轮 ASR 写成本地语言文字」的片段，结合整段读音线索和上下文还原目标语言，再处理真正的本地语言表达。正常本地表达、姓名与有歧义的片段不应仅因音近就被强行还原；纠错与翻译在同一次请求完成，不增加语音模型调用。
- 目标与本地语言书写系统不相交（如英/中）时，纯目标语言输入在本地即可判定，不调用 LLM、不增加延迟。
- 按下热键时即预先建立到翻译服务的连接；翻译短句时可在设置中关闭模型思考，进一步缩短延迟。

## 构建与运行

### Windows

需要 Windows 10 2004（19041）及以上、x64、.NET 10 SDK、Rust MSVC 工具链，以及 Visual Studio 2022 Build Tools 的 C++ 工具和 Windows SDK。

在 PowerShell 中运行：

```powershell
./scripts/build-windows.ps1 -Test -Install
```

脚本构建 Rust 动态库和 WinUI 3 客户端，执行 Rust 测试与真实 Windows 集成检查，再安装到 `%LOCALAPPDATA%\Programs\Sayso` 并创建开始菜单快捷方式。
发布产物在 `build/windows/`，自带 .NET 与 Windows App SDK 运行时；运行应用不需要开发工具或管理员权限。

- 默认按住**右 Ctrl**说话，松开结束；按住期间按 **T** 触发划词翻译。入口键、触发方式与命令路由均可在设置中修改。
- 关闭设置窗口后仍在系统托盘运行，托盘菜单可重新打开设置或退出。
- 在「模型配置」的「语音模型」「大模型」分区中填写服务配置；「测试翻译」调用真实服务验证配置。
- 使用 Windows Credential Manager 保存 API Key，普通设置与扩展数据在 `%LOCALAPPDATA%\Sayso`。
- 请在 Windows 设置中允许桌面应用访问麦克风；朗读使用系统已安装的对应语言语音。
- 双击 `.sayso` 文件可安装扩展；外部扩展启用前会显示其能力清单。
- 可在 Windows「已安装的应用」中卸载，卸载保留用户设置、扩展数据和凭据。卸载前请先从系统托盘退出应用。

集成测试使用独立临时目录和独立凭据命名空间，报告写入 `build/windows/smoke-results.json`。这些检查覆盖本地能力，云端识别与翻译需要另外配置真实服务后验证。

### macOS

需要 macOS 14+、Xcode 16+（Swift 6.2 工具链）与 Rust 工具链（rustup）。

```bash
cargo test --manifest-path core/Cargo.toml   # core 与内置扩展的测试
bash scripts/build-core.sh                   # 构建 core 并生成 Swift 绑定，之后可直接 swift build
INSTALL=1 bash scripts/bundle-app.sh         # 构建 core 与应用，打包并安装到 /Applications
open /Applications/Sayso.app
```

打包脚本默认用登录钥匙串中的 Apple Development 证书签名，签名身份在重新打包后保持不变，系统授权不会失效；
没有证书时退回 ad-hoc 签名，每次重新打包后需要重新授权辅助功能。

首次启动需要授予：

| 权限 | 用途 |
| --- | --- |
| 麦克风 | 录音 |
| 辅助功能 | 全局热键、读取选中文本、模拟 ⌘V 输入 |

然后在设置中填写：

- **语音识别**：火山引擎（豆包流式语音识别，只支持中英混说）、阿里云百炼千问 3.1 Message（短句实时输入，支持多语言提示和可选识别上下文）、Soniox 的 API Key，或任意 OpenAI 兼容的 `/audio/transcriptions` 服务。
- **翻译模型**：OpenAI、Claude、DeepSeek、火山方舟、百炼、Kimi、智谱、Gemini、OpenRouter、Ollama 或自定义 OpenAI 兼容服务。可用「测试翻译」验证。

数据位置（`~/Library/Application Support/Sayso/`）：

| 路径 | 内容 |
| --- | --- |
| `settings.json` | 设置 |
| `Extensions/` | 外部扩展，每个子目录一个 |
| `ExtensionData/<扩展 id>/` | 各扩展的数据，生词本为 `ExtensionData/sayso.wordbook/words.json` |

API Key 保存在登录钥匙串。
