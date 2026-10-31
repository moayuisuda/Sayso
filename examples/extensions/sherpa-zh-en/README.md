# 本地中英识别（sherpa-onnx）

Sayso 识别器扩展示例：用 [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) 的中英双语流式 Zipformer 模型在本机实时识别，
完全离线，CPU 即可实时运行，适合一句话里中英混说。

## 使用（拿到 .sayso 文件的用户）

1. 双击 `本地中英识别-arm64.sayso`（或在「设置 → 扩展」点「安装扩展…」选择它），确认安装。
2. 「设置 → 语音识别」的识别服务选「本地 Zipformer 中英双语（扩展）」。

扩展包里已经带好 Python、依赖和模型，不需要再装任何东西。

## 打包（扩展作者）

扩展文件夹要自带运行所需的一切，Sayso 安装时原样解压、不做任何下载。目标结构：

```
sherpa-zh-en/
├── extension.json
├── recognize.py
├── python/      可搬运的 Python 解释器
├── packages/    依赖
└── model/       识别模型
```

在扩展文件夹中准备这三样（以 Apple 芯片为例，Intel 机器换成 `x86_64` 版本并单独打包）：

1. **Python**：从 [python-build-standalone](https://github.com/astral-sh/python-build-standalone/releases) 下载
   `cpython-3.12.*-aarch64-apple-darwin-install_only.tar.gz`，解压后把其中的 `python` 文件夹放进扩展目录。
   这种发行版可以整体搬运，不像虚拟环境那样写死本机路径。
2. **依赖**：`./python/bin/python3 -m pip install --target packages sherpa-onnx numpy`
3. **模型**：下载
   [sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20.tar.bz2](https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20.tar.bz2)，
   解压后把文件夹改名为 `model`。

把整个文件夹放进 `~/Library/Application Support/Sayso/Extensions/`，在「设置 → 扩展」点「重新加载」并启用，
在「语音识别」中选用并试说几句。确认可用后，在扩展详情页点「导出…」得到 `.sayso` 文件，即可分发。

换用其他本地模型（Whisper、Paraformer 等）时，只需改写 `recognize.py` 中加载与解码的部分，协议见
[docs/extension-protocol.md](../../../docs/extension-protocol.md) 第 10 节。
