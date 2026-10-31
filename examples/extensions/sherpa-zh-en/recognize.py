"""Sayso 识别器扩展示例：用 sherpa-onnx 中英双语流式模型在本机实时识别。

协议（stdin / stdout 均为每行一条 JSON）：
  stdin : {"type":"start",...} → {"type":"audio","data":"<base64 pcm_s16le 16kHz mono>"}* → {"type":"end"}
  stdout: {"type":"partial","text":"…"}* → {"type":"final","text":"…"}，出错时 {"type":"error","message":"…"}

扩展文件夹自带运行所需的一切，整体搬到另一台同架构的 Mac 上即可运行：
  python/    可搬运的 Python 解释器
  packages/  依赖（pip install --target）
  model/     识别模型
"""

import base64
import json
import os
import re
import sys
from pathlib import Path

# 依赖装在扩展自己的 packages/ 中，不依赖系统环境。
sys.path.insert(0, str(Path(__file__).parent / "packages"))

import numpy as np  # noqa: E402
import sherpa_onnx  # noqa: E402

SAMPLE_RATE = 16000
MODEL_DIR = Path(os.environ.get("SHERPA_MODEL_DIR", Path(__file__).parent / "model"))


def emit(message: dict) -> None:
    print(json.dumps(message, ensure_ascii=False), flush=True)


def model_file(stem: str) -> str:
    """优先使用 int8 量化模型（更快），没有时使用原始模型。"""
    candidates = sorted(MODEL_DIR.glob(f"{stem}*.onnx"), key=lambda p: "int8" not in p.name)
    if not candidates:
        raise FileNotFoundError(f"{stem}*.onnx not found in {MODEL_DIR}")
    return str(candidates[0])


def load_recognizer() -> sherpa_onnx.OnlineRecognizer:
    return sherpa_onnx.OnlineRecognizer.from_transducer(
        tokens=str(MODEL_DIR / "tokens.txt"),
        encoder=model_file("encoder"),
        decoder=model_file("decoder"),
        joiner=model_file("joiner"),
        num_threads=2,
        sample_rate=SAMPLE_RATE,
        feature_dim=80,
        decoding_method="greedy_search",
    )


def tidy(text: str) -> str:
    """模型输出的英文为全大写：转为小写，句首与单独的 I 大写，中英文之间补空格。"""
    text = text.strip().lower()
    text = re.sub(r"(?<=[a-z0-9])(?=[\u4e00-\u9fff])|(?<=[\u4e00-\u9fff])(?=[a-z0-9])", " ", text)
    text = re.sub(r"\bi\b", "I", text)
    return text[:1].upper() + text[1:]


def main() -> None:
    try:
        recognizer = load_recognizer()
    except Exception as error:  # 模型缺失等问题直接报告给 Sayso，显示在候选框中
        emit({"type": "error", "message": f"Failed to load model: {error}"})
        return

    stream = recognizer.create_stream()
    last = ""

    def decode() -> str:
        while recognizer.is_ready(stream):
            recognizer.decode_stream(stream)
        return tidy(recognizer.get_result(stream))

    for line in sys.stdin:
        message = json.loads(line)
        if message["type"] == "audio":
            pcm = np.frombuffer(base64.b64decode(message["data"]), dtype=np.int16)
            stream.accept_waveform(SAMPLE_RATE, pcm.astype(np.float32) / 32768.0)
            text = decode()
            if text != last:
                last = text
                emit({"type": "partial", "text": text})
        elif message["type"] == "end":
            # 补一段静音并标记输入结束，让模型吐出最后几个字。
            stream.accept_waveform(SAMPLE_RATE, np.zeros(int(0.5 * SAMPLE_RATE), dtype=np.float32))
            stream.input_finished()
            emit({"type": "final", "text": decode()})
            return


if __name__ == "__main__":
    main()
