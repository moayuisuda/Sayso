import { runDictation } from "./dictation.js";

const { target, native } = sayso.languages;
const promptSuffix = `

The same JSON reply must also include "words": [{"target":"exact word from the rewritten sentence","native":"contextual meaning"}].
- Include the individual ${target.name} words in each translated or corrected span, in their order in the rewritten sentence. Segment words according to ${target.name} usage, including scripts without spaces. Keep contractions and compounds that count as one word.
- Copy each target word exactly as written in its segment. Supply a short contextual meaning in ${native.name}. Omit punctuation and unchanged target-language spans.
- Use an empty array if nothing was translated or corrected. Keep the "segments" format and all its rules.`;

export function input(event) {
  return runDictation(event, { promptSuffix, wordPairs });
}

function wordPairs({ words }, phrases) {
  if (!Array.isArray(words)) throw new Error("Translator returned no word list");
  // 词条必须来自实际发生变化的译文片段，模型列出的上下文词不能变成学习词条。
  const pairs = words.flatMap((word) => {
    if (typeof word?.target !== "string" || typeof word.native !== "string") {
      throw new Error("Translator returned an invalid word");
    }
    const target = word.target.trim();
    const native = word.native.trim();
    return target && native && phrases.some((phrase) => phrase.target.includes(target)) ? [{ target, native }] : [];
  });
  if (pairs.length === 0) throw new Error("Translator returned no words from the changed spans");
  return pairs;
}
