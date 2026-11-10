# Sayso 扩展协议（schemaVersion 4）

## 0. 设计原则

1. **一切皆扩展。** Sayso 只负责把语音、选中文本变成统一的文本输入，通过同一个 `input` 事件广播给所有已启用的订阅者。
   内置的「混说翻译」「互译」「生词本」与第三方扩展是同一种 JS 扩展，只是随应用发布（源码在 `core/plugins/`），
   它们使用宿主能力（调用翻译模型、写入前台应用）也只通过 `sayso.*` API。
2. **后处理与事件式。** 扩展按做的事分两种，内置扩展与第三方扩展都可以是任一种：
   - 混说翻译、互译和其他文本处理扩展订阅 `input`。扩展用 `event.input.via` 区分语音与划词，并可以在处理后广播 `translated`。
   - 生词本等扩展订阅 `translated`，附加按钮后以 `action` 接收点击。内置和第三方扩展使用相同的事件与能力。

## 1. 概念总览

```
 快捷键 ─▶ 输入（语音或选区）─▶ 广播 input ─▶ 已启用的订阅者（混说翻译 / 互译 / 第三方）
                     │                          │
                     ▼ hook: input              ▼ hook: translated → inserted
               ┌──────────── 事件式扩展（生词本、第三方）────────────┐
               │  订阅 hook ─▶ sayso.blocks 加内容，或在词条卡片上附加按钮 │
               │  用户点击按钮 ─▶ action ─▶ sayso.store / llm / fs …  │
               └──────────────────────────────────────────────────────┘
   候选框（各来源的块） ◀──── 渲染 ──── 扩展页签（按模板渲染扩展数据）
```

| 概念 | 说明 |
| --- | --- |
| **来源（source）** | 往候选框提供块的一方。Sayso 自身只有 `sayso.app`（应用状态提示）；每个启用的扩展也是一个来源 |
| **快捷键（shortcut）** | 启动 Sayso 的语音输入或读取前台应用的选区 |
| **hook** | 触发流程中各环节的广播事件：`input`（输入就绪）、`translated`（翻译完毕）、`inserted`（已写入），事件式扩展订阅它们 |
| **块（block）** | 候选框与页签里的最小界面单元，类型来自固定组件库 |
| **操作（action）** | 块上的可点击行为：朗读、复制、打开链接、写入、回调扩展、删除记录 |
| **数据（store）** | 每个扩展独有的记录集合，由 Sayso 托管存储 |
| **页签（tab）** | 扩展在设置窗口中的专属页面，按清单中的模板渲染其数据 |

## 2. 清单 `extension.json`

外部扩展安装在 Sayso 数据目录的 `Extensions/<目录>/` 下（macOS 为 `~/Library/Application Support/Sayso/Extensions/`），
目录内至少有 `extension.json` 与入口脚本，通常通过 `.sayso` 扩展包安装（见第 11 节）。完整示例见 [`examples/extensions`](../examples/extensions)。

```json
{
  "schemaVersion": 4,
  "id": "com.example.favorites",
  "name": "收藏夹",
  "description": "在翻译出的单词卡片上显示收藏按钮，收藏的词显示在「收藏」页签",
  "main": "main.js",
  "timeoutSeconds": 5,
  "events": ["translated", "action"],
  "capabilities": ["blocks", "store"],
  "tab": {
    "title": "收藏",
    "icon": "star",
    "collection": "favorites",
    "item": { "type": "term", "term": "{{word}}", "gloss": "{{meaning}}", "detail": "收藏 {{count}} 次", "language": "{{language}}" },
    "searchFields": ["word", "meaning"],
    "sort": { "field": "updatedAt", "order": "desc" },
    "itemActions": [
      { "type": "open", "label": "词典", "url": "https://dictionary.cambridge.org/search/direct/?q={{word}}" },
      { "type": "delete" }
    ]
  }
}
```

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `schemaVersion` | 是 | 协议版本，当前为 `4`；其他版本的扩展在扩展页显示为加载失败 |
| `id` | 是 | 全局唯一的反向域名标识，也是启停状态与数据目录的键；`sayso.` 前缀保留给内置扩展 |
| `name` / `description` | 是 / 否 | 扩展管理页显示 |
| `main` | 是 | 入口 ES 模块，相对扩展目录 |
| `timeoutSeconds` | 否 | 单次事件处理的超时，默认 10，取值 1–60；包括等待模型回复的时间 |
| `events` | 是 | 订阅的事件类型，至少一项，只会收到其中的事件 |
| `capabilities` | 否 | 声明的能力，见下表 |
| `tab` | 否 | 扩展页签，需同时声明 `store` 能力 |

| 能力 | 允许 |
| --- | --- |
| `blocks` | `sayso.blocks.*`，向候选框添加块，或在其他来源的块上附加按钮 |
| `store` | `sayso.store.*`，拥有自己的数据集合与页签；页签条目可用 `delete` 操作 |
| `insert` | 把文字写入前台应用：块上的 `insert` 操作（用户点击后），或后处理扩展处理 `input` 时调用 `sayso.insert` |
| `llm` | `sayso.llm`，用用户在设置中配置的翻译模型补全，扩展不需要自带 API Key |
| `publish` | `sayso.publish`，处理 `input` 时广播 `translated` hook |
| `fs` | `sayso.fs.*`，读写扩展目录与用户配置的路径（如 Obsidian 库） |
| `context` | `sayso.context.set`，设置提供给语音识别与翻译模型的背景 |
| `shell` | `sayso.shell.run`，执行跨平台脚本和本机程序；程序以当前用户权限运行，可读写文件和访问网络 |

外部扩展默认关闭。在「设置 → 扩展」启用时，Sayso 列出它订阅的事件与声明的能力，由用户确认。

### 语音与划词

| Sayso 输入 | 默认快捷键（macOS / Windows） | 用法 |
| --- | --- | --- |
| 语音 | 右 ⌥ / 右 Ctrl | 按住说话，松开结束 |
| 划词 | ⌥T / Ctrl+T | 选中文字后按下 |

「设置 → 通用 → 快捷键」管理 Sayso 的输入入口。识别完成或读到选区后，Sayso 向所有启用且订阅 `input` 的扩展广播同一条文本事件。

各订阅者独立运行。如果两个扩展都针对同一来源调用 `sayso.insert`，两次写入请求会依次交给宿主。用一个扩展替换另一个时，在「设置 → 扩展」启用替换版并停用原版。

## 3. 事件

扩展的入口模块按事件类型导出同名处理函数，可以是 `async` 函数。没有导出对应函数时，事件被忽略并记一条警告：

```js
export function action(event) { … }
export async function input(event) { … }
export function translated(event) { … }
export function inserted(event) { … }
export function focus(event) { … }
```

处理函数收到的事件对象中，公共字段与事件字段平铺在同一层：

```json
{
  "schemaVersion": 4,
  "type": "input",
  "id": "7C1E…",
  "timestamp": "2026-10-30T03:12:45Z",
  "languages": { "target": "en", "native": "zh" },
  "surface": { "id": "0C2B…", "blocks": [ … ] },
  "input": { "text": "I wish I had known it earlier.", "via": "selection" },
  "source": { "id": "com.apple.Safari", "name": "Safari" }
}
```

| 公共字段 | 说明 |
| --- | --- |
| `type` | 事件类型 |
| `id` | 事件唯一标识，可用于去重或作为记录 key |
| `timestamp` | ISO-8601（UTC，精确到秒） |
| `languages` | 目标语言 `target`（正在学的）与本地语言 `native`，BCP-47 代码：`en` `zh` `ja` `ko` `fr` `de` `es` `ru` |
| `surface` | 本次候选框。`blocks` 为此刻框中已有的块（id 带「来源 id/」前缀），扩展可读取并加以利用。来自页签的操作事件没有该字段 |

应用信息（`source`、`destination`）为 `{ "id", "name" }`，`id` 是平台的应用标识（macOS 为 Bundle ID），没有前台应用时为 `null`。

### 3.1 `input`：Sayso 广播准备好的文本

- `input.text`：输入文本，已去除首尾空白，非空。语音输入为识别原文，可能夹杂本地语言；`input.via` 为 `speech` 或 `selection`。
- `source`：触发时的前台应用。

### 3.2 `action`：用户点了该扩展提供的 `invoke` 操作（只发给提供按钮的扩展）

```json
"action": { "blockId": "sayso.dictation/@term-0", "payload": { "word": "serendipity" }, "origin": "candidate" }
```

- `blockId`：按钮在本扩展自己的块上时与输出时一致（不带前缀）；是用 `sayso.blocks.attach` 附加的按钮时，为被附加的块引用（`来源 id/块 id`），可直接用于再次附加。
- `origin` 为 `candidate`（候选框）或 `tab`（扩展页签）；来自页签时附带 `record`，即被点击条目对应的数据记录，且没有 `surface`。
- 来自候选框时，扩展输出的块继续显示在同一个候选框中，可用相同 id 覆盖原块，或对附加的按钮再次 `attach`（例如把「收藏」换成不可点击的「已收藏」）。
- 事件式扩展的典型流程：订阅某个 hook，在候选框中放一个带 `invoke` 操作的块，把之后要用的数据放进 `payload`；
  用户点击后在 `action` 中完成处理。`action` 事件不带 hook 的数据，需要的内容都要放在 `payload` 里。

### 3.3 `translated`：翻译完毕（hook，由后处理扩展广播）

```json
"input": { "text": "I have a 苹果.", "via": "speech" },
"output": "I have an apple.",
"pairs": [{ "target": "an apple", "native": "a 苹果", "card": "sayso.dictation/@term-0" }],
"context": "I have an apple.",
"card": "sayso.dictation/text",
"source": { "id": "com.apple.Notes", "name": "Notes" }
```

- `output`：译文。混说翻译为写入的整句（纯目标语言输入时即原句），互译为译文。
- `pairs`：「目标语言 · 本地语言」词对，与输入来源无关。混说翻译为每个翻译段一对，互译为整段一对；
  没有需要翻译的片段，或划词译为第三种语言时为空。
- `card`：Sayso 为该词对画出的词条卡片（`term` 块）的引用。广播前，Sayso 在候选框中为每个词条画一张卡片，归广播者所有；
  同一词条（忽略大小写与首尾标点）的词对共用一张卡片。没有候选框时为 `null`。
- `context`：词对所在的目标语言整句（混说翻译写入的句子）；互译为 `null`。
- `card`：展示整次翻译的整句卡片的引用，由广播者画出并指明（混说翻译为整句 `text` 块，互译为译文 `translation` 块）。没有词对时也有，没有候选框时为 `null`。
  附加在整句卡片上的按钮针对整次翻译，显示在候选框底部的操作栏（关闭按钮左侧），而不是整句下方。
- 每次触发都会广播。扩展不需要自己画词条或整句，用 `sayso.blocks.attach` 在卡片上附加自己的按钮即可：针对单个词条的挂到 `pair.card`（如生词本、收藏夹），针对整次翻译的挂到 `card`（如 Obsidian 记录整次翻译、语法讲解），也不需要关心翻译由谁、从哪种输入产生。

### 3.4 `inserted`：文字已写入前台应用（hook，由 Sayso 广播）

通过 `sayso.insert` 或块上的 `insert` 操作写入成功后，Sayso 广播给订阅了它的其他扩展：

```json
"text": "I have an apple.",
"destination": { "id": "com.apple.Notes", "name": "Notes" },
"delivery": "inserted",
"by": "sayso.dictation"
```

- `delivery` 为 `inserted`（已写入输入框）或 `copiedToClipboard`（没有可写入的位置，只放入剪贴板）。
- `by` 为发出写入的扩展。写入者自己不会收到。

### 3.5 `focus`：前台应用切换（hook，由 Sayso 广播）

切换到其他应用时广播给订阅者，Sayso 启动时也发一次。扩展刚启用，或它的「按应用配置」表刚被修改时，Sayso 会用当前应用再单独发给它一次。

```json
"app": { "id": "com.apple.dt.Xcode", "name": "Xcode" }
```

- `app` 的 `id` 跨次运行稳定（macOS 为 Bundle ID，Windows 为进程名），可以作为记录 key。回到 Sayso 自身或没有前台应用时 `app` 为 `null`。
- 同一应用内切换窗口不广播。事件没有候选框，处理时不能输出块。

## 4. `sayso` API

Sayso 在每个扩展的上下文中注入全局对象 `sayso`。块、数据、朗读、广播是**指令**，调用即生效、没有返回值；
模型、写入、读取数据、文件、脚本执行是**请求**（`sayso.store.list`、`sayso.llm`、`sayso.insert`、`sayso.shell.run` 返回 Promise）。
指令出错（内容无法识别、未声明能力）不会中断扩展，只被忽略并记为警告，显示在扩展管理页的最近一次执行状态中；请求出错（未声明能力、未配置模型、模型调用失败等）抛出异常。

| API | 能力 | 说明 |
| --- | --- | --- |
| `sayso.languages` | — | `{ target: { code, name }, native: { code, name } }`，`name` 为英文名，便于写进提示词 |
| `sayso.blocks.upsert(block)` | `blocks` | 在本次候选框中新增块，或按 `id` 更新本扩展已有的块 |
| `sayso.blocks.remove(id)` | `blocks` | 删除本扩展的一个块 |
| `sayso.blocks.attach(block, actions)` | `blocks` | 在本次候选框中其他来源的块（`来源 id/块 id`，如 `translated` 事件的 `card` 与词对的 `card`）上附加本扩展的按钮，替换本扩展之前附加的那组；`actions` 为空数组时撤下。按钮显示在块的右侧，排在块自身的操作之后（附加在整句卡片上的显示在底部操作栏），点击后回调本扩展；块被移除时一并移除 |
| `sayso.store.put(collection, key, record)` | `store` | 整条写入（覆盖扩展字段） |
| `sayso.store.patch(collection, key, { set, increment })` | `store` | 局部更新，记录不存在时创建；`increment` 对数字字段做加法，例如 `{ count: 1 }` |
| `sayso.store.delete(collection, key)` | `store` | 删除一条记录 |
| `await sayso.store.list(collection)` | `store` | 读取集合中的全部记录（含本次已执行的写入） |
| `sayso.speak(text, language)` | — | 自动朗读；用户关闭了「自动朗读」时不发声 |
| `await sayso.llm(system, user)` | `llm` | 用用户配置的模型与思考强度补全，返回模型输出的文本；当前有背景（见 `sayso.context.set`）时，core 会把它附在 `system` 之后。未配置模型或调用失败时抛出异常 |
| `await sayso.insert(text)` | `insert` | 写入触发输入时的前台应用，返回 `"inserted"` 或 `"copiedToClipboard"`；只能在处理 `input` 时使用 |
| `sayso.publish({ type: "translated", output, pairs, context, card })` | `publish` | 处理 `input` 时广播 `translated` hook，见下文 |
| `sayso.context.set(text)` | `context` | 替换提供给语音识别与翻译模型的背景，下一次录音与模型请求生效；超过 400 字时截断。只保存在内存中，多个扩展设置时以最后一次为准，声明了该能力的扩展被停用时清空 |
| `sayso.text.mayContainNative(text)` | — | 文本是否可能夹杂本地语言；纯目标语言的文本可以跳过模型调用 |
| `sayso.fs.readText(path)` / `writeText(path, text)` / `appendText(path, text)` / `exists(path)` / `mkdir(path)` | `fs` | 同步读写 UTF-8 文本文件；相对路径相对扩展目录，`~/` 展开为用户主目录；`mkdir` 会创建中间目录 |
| `await sayso.shell.run(script, options?)` | `shell` | 执行跨平台脚本，返回 `{ exitCode, stdout, stderr }`，详见下文 |
| `console.log(…)` 等 | — | 输出的末尾 4KB 显示在扩展管理页的「最近一次运行」中 |

- 块 id 只在本扩展内唯一，Sayso 按来源分组，不同扩展之间不会冲突。以 `@` 开头的块 id 留给 Sayso 在扩展分组中代画的块（如词条卡片 `@term-0`），扩展不能用它新增或移除块，只能附加按钮。
- 集合名只允许字母、数字、`_`、`-`。记录的 `key`、`createdAt`、`updatedAt` 由 Sayso 维护，扩展写入同名字段会被忽略。
- 数据写入由 Sayso 串行、原子落盘，多个事件并发处理时不会写坏数据。

### 跨平台 shell

脚本由随应用打包的 `sayso-shell` 进程使用 `deno_task_shell` 解析和执行，macOS 与 Windows 共用同一套 shell 语法，无需安装 Deno、Bash 或 PowerShell。
支持管道、重定向、环境变量、`&&` / `||`，以及 `cat`、`head`、`echo`、`mkdir`、`cp`、`mv` 等内置命令。
它支持部分 sh/bash 语法，完整语法见 [Deno task 文档](https://docs.deno.com/runtime/reference/cli/task/)；Git、Python 等外部程序仍需本机安装并能通过 PATH 或路径找到。

清单中声明 `"capabilities": ["shell"]` 后即可调用：

```js
const result = await sayso.shell.run("cat | head -n 1", {
  stdin: event.input.text,
  timeoutMs: 3000,
});
if (result.exitCode !== 0) throw new Error(result.stderr);
console.log(result.stdout);
```

| `options` 字段 | 默认值 | 说明 |
| --- | --- | --- |
| `cwd` | 扩展目录 | 工作目录；相对路径相对扩展目录，`~/` 展开为用户主目录；目录必须存在 |
| `env` | `{}` | 当前进程环境变量的覆盖项，值为字符串；只影响本次调用 |
| `stdin` | `""` | 标准输入文本，最多 1 MiB；写完关闭输入，不读取应用进程的键盘输入 |
| `timeoutMs` | 本次事件剩余时间 | 正整数，范围 1–60000；不能延长清单 `timeoutSeconds` 对整个事件的限制，排队等待也计入超时 |

- `stdout`、`stderr` 分别收集并按 UTF-8 解码，无效字节替换为 `�`；两者合计最多 1 MiB，超限时停止执行并抛出异常。
- 非零退出码正常返回，由扩展决定如何处理；语法错误、参数错误、超时和执行基础设施错误会使 Promise 拒绝。
- 全应用同时最多执行 4 次 shell 调用，超过时异步排队，不阻塞界面。
- 事件结束、超时、候选框关闭或宿主取消该事件任务时，仍在执行的脚本会收到停止信号；脚本中的后台命令也受本次调用生命周期约束，不支持持久后台任务。
- `shell` 是执行本机程序的完整权限，不受扩展目录或 `fs` 能力限制。用户输入应通过 `stdin` 或 `env` 传入，避免把输入直接拼接为脚本。

### 广播 hook

`translated` 由后处理扩展在翻译完毕时用 `sayso.publish` 广播：

```js
sayso.publish({ type: "translated", output: "意外发现", pairs: [{ target: "serendipity", native: "意外发现" }], context: null, card: "translation" });
```

- `card` 为广播者自己展示整次翻译的块 id（不带前缀），必须已在候选框中；Sayso 把它换成引用填入事件。
- 只能广播 `translated`；`input`、`inserted` 由 Sayso 发出，`action` 是定向回调。
- Sayso 为词对画出词条卡片并填写各词对的 `card`，补全事件的 `id`、`timestamp`、`languages`、`surface`，并从触发的输入补全 `input` 与 `source`，
  发给除广播者以外订阅了该 hook 的已启用扩展，它们输出的块显示在同一个候选框中。
- 只能在处理 `input` 时广播，避免循环。第三方后处理扩展广播的 `translated` 与内置翻译的一样处理，订阅者不需要区分来源。

### 运行环境

- 入口与它导入的模块都是 ES 模块，只能用相对路径（`./`、`../`）导入扩展目录内的文件；JS 上下文没有 Node.js、浏览器 API 或直接网络 API。声明 `shell` 后可以通过本机程序访问网络。
- 每个事件新建上下文，模块顶层代码每次都会重新执行；内存上限 64MB。
- 超过 `timeoutSeconds` 时处理被中断（包括同步死循环），记为超时。处理函数抛出的异常记为失败，原因显示在候选框与扩展管理页。

### 数据文件格式

数据位于 Sayso 数据目录的 `ExtensionData/<扩展 id>/<collection>.json`，为记录数组，每条记录是扁平对象：

```json
[
  { "key": "en|serendipity", "createdAt": "2026-10-30T03:12:50Z", "updatedAt": "2026-10-30T03:12:50Z",
    "word": "serendipity", "meaning": "意外发现", "language": "en", "count": 1 }
]
```

## 5. 块（组件库）

所有块共有字段：`id`（必填）、`type`（必填）、`actions`（可选，操作数组）。

| `type` | 字段 | 渲染 |
| --- | --- | --- |
| `text` | `runs: [{text, style?, action?}]`，`role?: primary \| secondary` | 一段可换行的富文本。`style` 可取 `normal`、`highlight`（荧光高亮）、`muted`；`action` 引用本块 `actions` 中某个操作的 `id`，使该片段可点击 |
| `term` | `term`，`gloss?`，`language`，`detail?` | 词条：目标语言词句 · 释义，点击即朗读；`detail` 为一行小字（音标、词性、次数等）。翻译出的词条卡片由 Sayso 统一画出，扩展通常只往上附加按钮 |
| `card` | `title`，`subtitle?`，`body?` | 标题、副标题和多行正文 |
| `status` | `message`，`icon?`（SF Symbol 名称），`tone?: info \| success \| warning \| error` | 一行状态提示 |

未被文本片段引用的操作显示为按钮：单行的 `term`、`status` 块在同一行右侧，其他块在下方；其他扩展附加的按钮排在后面。可选字符串为空时不显示（便于模板引用缺失字段）。

内置扩展「混说翻译」产生的块示例（即其他扩展收到的事件中 `surface.blocks` 的内容）：

```json
[
  { "id": "sayso.dictation/text", "type": "text", "role": "primary",
    "runs": [ { "text": "I have " }, { "text": "an apple", "style": "highlight", "action": "speak-0" }, { "text": "." } ],
    "actions": [ { "id": "speak-0", "type": "speak", "text": "an apple", "language": "en" } ] },
  { "id": "sayso.dictation/@term-0", "type": "term", "term": "an apple", "gloss": "a 苹果", "language": "en" }
]
```

其中 `@term-0` 是 Sayso 按广播的词对画出的词条卡片。互译扩展产生 `sayso.selection/translation`（译文）、`sayso.selection/original`（原文）两个 `text` 块，涉及目标语言时另有 Sayso 画出的 `sayso.selection/@term-0`。

## 6. 操作

| `type` | 字段 | 能力 | 行为 |
| --- | --- | --- | --- |
| `speak` | `text`，`language` | — | 用系统语音朗读 |
| `copy` | `text` | — | 复制到剪贴板 |
| `open` | `url` | — | 用默认浏览器打开。仅限 `http`/`https`；空格、中文等非法字符会自动编码 |
| `insert` | `text` | `insert` | 写入前台应用的输入框 |
| `invoke` | `payload?` | — | 以 `action` 事件回调本扩展 |
| `delete` | — | `store` | 仅在页签条目上显示，删除对应记录（会二次确认） |

所有操作都可带 `id`（供文本片段引用）、`label`（按钮文字，缺省时按类型显示）与 `disabled`（为 `true` 时按钮不可点击，用于展示「已加入」等完成状态）。

## 7. 扩展页签

声明了 `tab` 的已启用扩展在设置窗口中拥有一个独立页签。Sayso 直接读取该扩展的数据集合，按模板渲染，打开页签时**不会**运行扩展：

- `item`：块模板（不含 `id`，条目 id 取记录的 `key`），字符串中用 `{{字段名}}` 引用记录字段，也可引用 `key`、`createdAt`、`updatedAt`。只做文本替换，不支持逻辑；缺失字段替换为空串。
- `searchFields`：页签搜索框匹配的字段。
- `sort`：排序字段与方向（`asc` / `desc`）；数字按数值，其余按文本。
- `itemActions`：每个条目上的操作，同样可用模板。条目上的 `invoke` 以 `origin: "tab"` 回调扩展，并附带该条记录。

页签也可以不声明 `item`，改为声明 `appTable`（两者必须二选一），由 Sayso 渲染成可编辑的「按应用配置」表，打开和编辑时同样不运行扩展：

```json
"tab": {
  "title": "应用背景",
  "collection": "contexts",
  "appTable": { "placeholder": "例如：我是软件工程师", "maxLength": 400 }
}
```

- 顶部是固定的默认行；下方每行左边选一个正在运行的应用，右边填文本，末尾的加号添加新行。一个应用只能出现在一行。
- 记录字段为 `appId`、`appName`、`text`，默认行的 `key` 为 `default`。扩展用 `sayso.store.list` 读取。
- 表被修改后，Sayso 会向订阅了 `focus` 的本扩展发一次 `focus` 事件，让修改立即生效。

## 8. 候选框的组成与显示时长

一次按快捷键对应一个候选框：

1. 录音时显示「正在聆听 · 命令名」与实时识别文字。输入准备好后，在扩展输出第一个块之前显示识别原文或选中文本与处理进度。
2. 扩展的块按到达顺序分组追加；外部扩展的分组带扩展名称，内置扩展不显示名称。
3. 有扩展仍在处理时，候选框保持显示并提示「处理中」（与识别阶段是同一个状态），块到达后立即出现。
4. 右上角有关闭按钮，随时可手动关闭。内容超过一定高度时可滚动（录音中的实时文字自动跟随最新内容）。鼠标悬停时不会自动隐藏；内容最后一次变化后，按内容停留一段时间再隐藏：语音命令 1.5 秒、选中文本命令 8 秒、错误 3 秒；含高亮文本（如混说翻译的译文）时至少 5 秒；有等待点击的 `invoke` 按钮（如「加入生词本」，含附加的按钮，不含 `disabled` 的按钮）时至少 15 秒。

## 9. 内置扩展

内置扩展的源码在 [`core/plugins`](../core/plugins)，编译时嵌入 core，与第三方扩展使用同一套 API，也是编写扩展的参考实现。

### 9.1 混说翻译 `sayso.dictation`

- 订阅 `input`，处理 `via: "speech"`；能力为 `blocks`、`publish`、`insert`、`llm`。
- 用目标语言说话，不会的词用本地语言说：用 `sayso.llm` 生成完整的目标语言句子并调整必要的语法、语序，用 `sayso.insert` 写入前台应用。输出片段通过原文单元的编号范围建立对应关系，引用可重排，须覆盖原文词语且不能重复，独立标点可随整句重排删除或合并；原文无需由模型重抄。高亮和词对由实际文字变化推导，忽略共同前后文及纯大小写、空白、标点变化，模型不再提供语言标注字段。
- `main.js` 负责执行流程，`prompt.js` 负责通用提示词组装，`languages.js` 负责语言资料及语言对规则，`transcript.js` 负责无损的原文引用与结果组装。只注入当前目标语言、本地语言的资料，并由等义例句生成当前语言对的完整分段示例。内置模块通过受限的嵌入式 ES 模块加载器导入，两端共用同一份源码。
- 输出 `text` 块（id `text`，译文高亮、点击朗读），必要时附提示块（`clipboard`、`notice`），朗读每个翻译段，最后广播 `translated`（`output`、`context` 均为写入的整句，`card` 为 `text`），Sayso 随之为每个翻译段画出词条卡片。
- 未配置翻译模型或翻译失败时原样写入识别结果并提示原因，说的话不丢。

### 9.2 互译 `sayso.selection`

- 订阅 `input`，处理 `via: "selection"`；能力为 `blocks`、`publish`、`llm`。
- 输入为本地语言则译为目标语言，其他语言译为本地语言；输出译文（`translation`）、原文（`original`）两个 `text` 块。
- 涉及目标语言时朗读目标语言一侧。翻译完毕广播 `translated`（`output` 为译文，`context` 为 `null`，`card` 为 `translation`）；第三种语言（如目标语言为日语时选中英文）不朗读，词对为空。

### 9.3 生词本 `sayso.wordbook`

- 事件式扩展：订阅 `translated`、`action`，能力为 `blocks`、`store`，拥有「生词本」页签（集合 `words`）。
- 不自动记录，也不画任何块：在每张词条卡片上附加「加入生词本」按钮，不因长度过滤句子；有多个词条时在整句卡片上附加「全部加入生词本」（显示在底部操作栏）。
- 按钮是 `invoke` 操作，载荷中带上要加入的词条、本次全部词条与整句卡片引用，扩展本身不保存候选状态。点击后以 `sayso.store.patch` 写入（键为「语言|小写词条」），`increment: { count: 1 }`；有 `context` 时记录为例句，没有时不覆盖已有例句。
- 记录中保存最近加入时的候选框 id，同一候选框中每个词条只计一次，因此先单独加入、再「全部加入」不会重复计数。
- 已加入的词条把按钮换成不可点击的「已加入生词本（第 3 次）」；本次的词条都已加入后（无论逐个还是一次全部加入），整句卡片上的按钮换成不可点击的「已全部加入生词本」。

### 9.4 应用背景 `sayso.context`

- 事件式扩展：订阅 `focus`，能力为 `store`、`context`，拥有「应用背景」页签（集合 `contexts`，`appTable` 表）。
- 切换应用时，背景为默认行文本加上该应用这一行的文本（换行分隔，默认行在前）；当前应用不在表中或这一行文本为空时，只用默认行。

## 10. 本地语音服务

语音识别由 core 的服务适配层处理，扩展接收识别完成后的文本事件，不接收音频，也不启动模型进程。
本地模型由用户独立启动服务，在「设置 → 模型配置 → 语音模型」选择「OpenAI 兼容 / 本地服务」：

- Base URL 填写服务地址，例如 `http://127.0.0.1:8000/v1`。
- 模型填写服务实际提供的名称，无需认证时 API Key 留空。
- 服务需支持 `POST /audio/transcriptions`：接收 multipart 表单中的 WAV 文件与模型名，返回 `{"text":"转写结果"}`。
- Sayso 在录音结束后上传整段音频；本地服务的安装、启动与停止由用户管理。

## 11. 扩展包（`.sayso`）

扩展包是扩展文件夹的 zip 归档，用于分享与安装。

- **原样打包、原样安装。** 扩展包包含扩展文件夹中的全部文件：清单、JS 模块与资源。
  Sayso 安装时只解压，不执行下载或安装步骤；JS 扩展由内嵌的 QuickJS 运行，在各平台共用同一份代码。
- **用户数据不进扩展包。** 扩展通过 `sayso.store` 保存的数据位于 Sayso 数据目录的 `ExtensionData/<扩展 id>/`，
  不在扩展文件夹内，导出扩展不会带出用户数据。

| 操作 | 方式 |
| --- | --- |
| 导出 | 「设置 → 扩展」选中外部扩展，点「导出…」 |
| 安装 | 双击 `.sayso` 文件，或在「设置 → 扩展」点「安装扩展…」 |
| 删除 | 扩展详情页点「删除…」；扩展保存的数据保留 |

安装前，Sayso 显示扩展的名称、说明、大小、订阅的事件与申请的能力，并提示扩展包含会在本机运行的代码；
用户确认后，扩展解压到 `Extensions/<扩展 id>/` 并启用。已安装同一 id 的扩展时整体替换，数据保留。
用户确认即表示信任其中的代码，因此解压时不保留下载带来的隔离标记，否则扩展程序会被系统阻止运行。

JS 扩展只能通过 `sayso.*` 使用宿主能力，能力声明即其边界（`fs` 能力可读写用户的文件）。
只安装来自可信来源的扩展包。
