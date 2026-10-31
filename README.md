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
- **识别器**：扩展可以提供流式语音识别器（例如本地模型），在「设置 → 语音识别」中选用，录音时实时出字。
- **能力**：扩展声明需要的能力（`blocks` / `store` / `insert` / `llm` / `publish` / `fs` / `audio`），外部扩展启用时由用户确认。
  `llm` 让扩展直接用你配置的翻译模型，不需要自带 API Key；内置的语音翻译、划词翻译也只用这些能力，外部扩展同样能实现。

扩展是一个 JS 模块（`main.js`），按事件类型导出处理函数，通过全局的 `sayso.*` API 使用宿主能力；由 Sayso 内嵌的 QuickJS 运行，
不依赖平台，macOS 与 Windows 共用同一份扩展。只有识别器以独立进程运行，可以用任何语言编写。协议与数据结构见 **[docs/extension-protocol.md](docs/extension-protocol.md)**。

**分享扩展**：在「设置 → 扩展」点「导出…」，扩展文件夹原样打包成一个 `.sayso` 文件（包含代码、模型、运行环境，不含你的数据）；
对方双击即可安装，不需要终端，也不会再下载任何东西。自带运行环境的识别器按平台与芯片架构分别发布。

示例扩展复制到 `~/Library/Application Support/Sayso/Extensions/` 后在「设置 → 扩展」中重新加载并启用即可试用：

- [`examples/extensions/favorites`](examples/extensions/favorites)：语音翻译、划词翻译出单词时显示「收藏」按钮，收藏的词有自己的页签。
- [`examples/extensions/explain`](examples/extensions/explain)：讲解一句外语的语法，用 `sayso.llm` 调用你配置的模型；路由设为选区即讲解选中的句子。
- [`examples/extensions/todo`](examples/extensions/todo)：命令「添加待办」，把说的话或选中的文字记为待办，显示在「待办」页签，点「完成」删除；
  在「设置 → 扩展」中为它设路由键（如 D）并选择输入来源，或设为「直接」与语音翻译同时触发。
- [`examples/extensions/obsidian`](examples/extensions/obsidian)：把 Sayso 每次写入的文字追加到 Obsidian 当天的笔记，
  启用前在 `config.json` 中填写库目录。
- [`examples/extensions/sherpa-zh-en`](examples/extensions/sherpa-zh-en)：识别器扩展，用 sherpa-onnx 中英双语流式模型离线实时识别；
  其 README 说明如何把 Python、依赖与模型放进扩展文件夹并导出为可直接分发的 `.sayso`。

## 架构

```
core/                     Rust core（macOS 与 Windows 共用），通过 UniFFI 暴露给各平台宿主
├── sayso-core/src/
│   ├── protocol/           扩展协议：语言、块、操作、事件、清单与页签模板
│   ├── plugin/             JS 扩展运行时（QuickJS）、`sayso.*` API、扩展目录加载
│   ├── session/            会话状态机：路由 → 录音识别或读取选区 → 命令；候选框内容、停留时长、块操作
│   ├── asr/                识别来源注册表；火山引擎 / 千问 / Soniox（流式 WebSocket）、OpenAI 兼容（批量）、识别器子进程
│   ├── llm/                OpenAI 兼容 / Anthropic 客户端、服务预设、思考强度、连接预热
│   ├── extensions.rs       扩展启停、命令路由、广播对象
│   ├── store.rs            扩展数据存储（串行、原子落盘）
│   ├── settings.rs         设置（settings.json；密钥经平台安全存储）
│   ├── package.rs          `.sayso` 扩展包
│   └── platform.rs         平台适配层接口：录音、选区、写入、朗读、安全存储与状态推送
└── plugins/              内置扩展「语音翻译」「划词翻译」「生词本」（JS，编译时嵌入）
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
```

**混说翻译的设计要点**

- LLM 直接给出完整的目标语言句子，并按段标注哪些是由本地语言翻译而来：
  `{"segments":[{"text":"I have "},{"text":"an apple","source":"a 苹果"},{"text":"."}]}`。
  译文可以连带改写相邻的冠词、介词、主谓一致等（`a 苹果` → `an apple`），其余目标语言措辞保持不变。
- 每个翻译块的 `source` 必须能在识别原文中按顺序找到，防止模型编造；否则视为失败，原样输入识别结果，并在候选框提示原因，保证说的话不丢。
- 目标与本地语言书写系统不相交（如英/中）时，纯目标语言输入在本地即可判定，不调用 LLM、不增加延迟。
- 按下热键时即预先建立到翻译服务的连接；翻译短句时可在设置中关闭模型思考，进一步缩短延迟。

## 构建与运行

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

- **语音识别**：火山引擎（豆包流式语音识别，只支持中英混说）、阿里云百炼千问实时识别（自动识别语言，支持中日等多语言混说）、Soniox 的 API Key，或任意 OpenAI 兼容的 `/audio/transcriptions` 服务。
- **翻译模型**：OpenAI、Claude、DeepSeek、火山方舟、百炼、Kimi、智谱、Gemini、OpenRouter、Ollama 或自定义 OpenAI 兼容服务。可用「测试翻译」验证。

数据位置（`~/Library/Application Support/Sayso/`）：

| 路径 | 内容 |
| --- | --- |
| `settings.json` | 设置 |
| `Extensions/` | 外部扩展，每个子目录一个 |
| `ExtensionData/<扩展 id>/` | 各扩展的数据，生词本为 `ExtensionData/sayso.wordbook/words.json` |

API Key 保存在登录钥匙串。