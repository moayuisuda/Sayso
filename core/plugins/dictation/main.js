// 内置扩展「语音翻译」：用目标语言说话，不会的词用本地语言说，翻译成完整的目标语言句子后写入前台应用。
//
// 命令触发 → 混说翻译（sayso.llm）→ 写入（sayso.insert）→ 输出文本块与提示 → 朗读翻译块 → 广播 translation，
// 生词本等扩展据此记录。翻译失败时原样写入识别结果，保证说的话不丢。

import { createDictationPrompt } from "./prompt.js";
import { referenceTranscript, assembleTranslation } from "./transcript.js";

const { target, native } = sayso.languages;
const chinese = native.code === "zh";

export async function command(event) {
  if (event.command.id !== "translate") return;
  const { segments, notice } = await compose(event.input.text);
  const output = segments.map((segment) => segment.text).join("");
  sayso.blocks.upsert(textBlock(segments));
  try {
    const delivery = await sayso.insert(output);
    if (delivery === "copiedToClipboard") {
      const message = chinese ? "未能写入目标应用，已复制到剪贴板" : "Could not insert into the target app; copied to clipboard";
      sayso.blocks.upsert(statusBlock("clipboard", message, "info", "doc.on.clipboard"));
    }
  } catch (error) {
    const prefix = chinese ? "输入失败：" : "Insertion failed: ";
    sayso.blocks.upsert(statusBlock("insertion", prefix + error.message, "error", "exclamationmark.circle"));
  }
  if (notice) sayso.blocks.upsert(statusBlock("notice", notice, "warning", "exclamationmark.circle"));

  const pairs = segments.filter(isTranslation).map((segment) => ({ target: segment.text, native: segment.source }));
  for (const pair of pairs) sayso.speak(pair.target, target.code);
  if (pairs.length > 0) sayso.publish({ type: "translation", pairs, context: output });
}

/**
 * 把混说识别结果转换为目标语言输出：`[{ text, source? }]`，带 source 的是翻译块。
 * 纯目标语言输入直接输出，不产生模型调用和延迟；翻译失败时原样输出并给出提示。
 */
async function compose(transcript) {
  const text = transcript.trim();
  if (!sayso.text.mayContainNative(text)) return { segments: verbatim(text) };
  try {
    const units = referenceTranscript(text);
    const reply = await sayso.llm(createDictationPrompt(sayso.languages), JSON.stringify({ transcript: text, units }));
    return { segments: assembleTranslation(units, reply) };
  } catch (error) {
    const prefix = chinese ? "翻译失败，保留原始识别结果：" : "Translation failed; keeping the original transcript: ";
    return { segments: verbatim(text), notice: prefix + error.message };
  }
}

function verbatim(text) {
  return text ? [{ text }] : [];
}

function isTranslation(segment) {
  return segment.source !== undefined;
}

/** 整句为一个 text 块，翻译出的部分高亮、点击朗读。词条列表由生词本扩展提供。 */
function textBlock(segments) {
  const actions = [];
  const runs = segments.map((segment) => {
    if (!isTranslation(segment)) return { text: segment.text };
    const id = `speak-${actions.length}`;
    actions.push({ id, type: "speak", text: segment.text, language: target.code });
    return { text: segment.text, style: "highlight", action: id };
  });
  return { id: "text", type: "text", role: "primary", runs, actions };
}

function statusBlock(id, message, tone, icon) {
  return { id, type: "status", message, tone, icon };
}
