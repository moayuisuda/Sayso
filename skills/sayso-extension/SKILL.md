---
name: sayso-extension
description: 为 Sayso 编写、调试和打包 JavaScript 扩展。用于处理语音或划词文本、替换混说翻译、扩展候选框或创建扩展页签。
---

# 开发 Sayso 扩展

从 [扩展协议](../../docs/extension-protocol.md) 查阅所需事件、API 和块结构。相对链接以本文件在 Sayso 仓库中的位置为准。开发独立扩展时，从用户提供的 Sayso 仓库读取参考资料，把代码写到用户指定的扩展目录。

## 按需求选择事件

| 需求 | 入口 | 参考实现 |
| --- | --- | --- |
| 处理识别或划词后的文本 | 订阅 `input`，读取 `event.input.text`，按需要筛选 `event.input.via` 的 `speech` 或 `selection` | [混说翻译](../../core/plugins/dictation) · [划词互译](../../core/plugins/selection) |
| 给翻译结果加按钮 | 订阅 `translated`，用 `sayso.blocks.attach` 把按钮挂到整句 `event.card` 或词条 `event.pairs[].card`，并用 `invoke` 回调 `action(event)` | [讲解语法](../../examples/extensions/explain) |
| 保存内容并在设置里查看 | 通过 `sayso.store` 写入数据，并在清单的 `tab` 声明页签和集合 | [收藏夹](../../examples/extensions/favorites) |
| 切换应用时更新背景 | 订阅 `focus`，读取 `event.app` 并更新背景 | [应用背景](../../core/plugins/context) |

语音和划词使用同一个 `input` 广播。快捷键由 Sayso 管理，所有已启用的订阅者都会收到事件。两个扩展都对同一输入调用 `sayso.insert` 时，宿主会收到两次写入请求。替换内置混说翻译时，用自己的扩展 ID，试用时启用替换版并停用原版。

## 实现

- 交付包含 `extension.json` 和入口 ES 模块的扩展文件夹。按协议声明 `schemaVersion`、`main` 和 `events`，导出与事件同名的处理函数。使用唯一的反向域名 ID，`sayso.` 前缀留给内置扩展。只声明实际使用的能力。
- Sayso 在 QuickJS 中为每个事件创建新的上下文。使用扩展目录内的相对 ES 模块和宿主注入的 `sayso.*` API。跨事件数据用 `sayso.store`，例如把列表页数据放在清单指定的集合中。调用已配置的翻译模型用 `sayso.llm`。运行时没有 Node.js、浏览器 API 或直接网络 API。
- `sayso.insert` 和 `sayso.publish` 只能在 `input` 中调用。处理 `sayso.insert` 返回的 `copiedToClipboard`，让界面准确反映写入结果。要和生词本联动，先渲染整句块，再广播 `translated`，传入自己的整句块 ID 和译文、原文词对，由 Sayso 生成词条卡片。
- 若任务确实需要本机文件或程序，按协议使用 `fs` 或 `shell`。面向 macOS 和 Windows 时，显式说明外部程序依赖并验证两端的路径与命令。通过 `stdin` 或 `env` 向脚本传入用户文本。

## 验证和交付

1. 检查清单、事件处理函数、模块路径和能力声明是否一致。静态检查 JavaScript 时按 ES 模块解析。
2. 在 Sayso 中验证时，把扩展文件夹放入[扩展目录](../../README.md#扩展)，在「设置 → 扩展」重新加载并启用。用预期输入触发，检查候选框或写入结果，并查看扩展的最近执行记录。若扩展只处理一种输入，也用另一种输入确认筛选生效。交付时区分静态检查和宿主验证。
3. 需要分享时，在扩展详情选择「导出…」生成 `.sayso`，也可以将清单、入口及其依赖放在 ZIP 根目录后使用 `.sayso` 扩展名。发布前检查包内文件，确保没有密钥和本地测试数据。
