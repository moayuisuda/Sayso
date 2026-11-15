// 混说翻译中间件：处理语音文本，返回译文供 Sayso 写入，发布高亮和学习词对。

import { createDictationPrompt } from "./prompt.js";
import { referenceTranscript, readTranslationReply, assembleTranslation } from "./transcript.js";

const { target, native } = sayso.languages;
const chinese = native.code === "zh";
/** 整句卡片：写入的整句，翻译出的部分高亮。 */
const TEXT = "text";

export function input(event) {
  return runDictation(event);
}

export async function runDictation(event, { promptSuffix = "", wordPairs } = {}) {
  if (event.input.via !== "speech") return;
  const { segments, notice, reply } = await compose(event.input.text, promptSuffix);
  const output = segments.map((segment) => segment.text).join("");
  sayso.blocks.upsert(textBlock(segments));
  if (notice) sayso.blocks.upsert(statusBlock("notice", notice, "warning", "exclamationmark.circle"));

  const phrases = segments.filter(isTranslation).map((segment) => ({ target: segment.text, native: segment.source }));
  for (const phrase of phrases) sayso.speak(phrase.target, target.code);
  let pairs = phrases;
  if (wordPairs && phrases.length > 0) {
    try {
      pairs = wordPairs(reply, phrases);
    } catch (error) {
      pairs = [];
      const prefix = chinese ? "拆词失败：" : "Word breakdown failed: ";
      sayso.blocks.upsert(statusBlock("words", prefix + error.message, "warning", "exclamationmark.circle"));
    }
  }
  // 即使词对为空，订阅者也能在整句卡片上附加按钮。
  sayso.publish({ type: "translated", output, pairs, context: output, card: TEXT });
  return output;
}

/**
 * 把混说识别结果转换为目标语言输出：`[{ text, source? }]`，带 source 的是翻译块。
 * 纯目标语言输入直接输出，不产生模型调用和延迟；翻译失败时原样输出并给出提示。
 */
async function compose(transcript, promptSuffix) {
  const text = transcript.trim();
  if (!sayso.text.mayContainNative(text)) return { segments: verbatim(text) };
  try {
    const units = referenceTranscript(text);
    const prompt = createDictationPrompt(sayso.languages) + promptSuffix;
    const reply = readTranslationReply(await sayso.llm(prompt, JSON.stringify({ transcript: text, units })));
    return { segments: assembleTranslation(units, reply.segments), reply };
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

/** 整句为一个 text 块，翻译出的部分高亮、点击朗读。词条卡片由 Sayso 按广播的词对画出。 */
function textBlock(segments) {
  const actions = [];
  const runs = segments.map((segment) => {
    if (!isTranslation(segment)) return { text: segment.text };
    const id = `speak-${actions.length}`;
    actions.push({ id, type: "speak", text: segment.text, language: target.code });
    return { text: segment.text, style: "highlight", action: id };
  });
  return { id: TEXT, type: "text", role: "primary", runs, actions };
}

function statusBlock(id, message, tone, icon) {
  return { id, type: "status", message, tone, icon };
}
