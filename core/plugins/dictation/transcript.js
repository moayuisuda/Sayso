/** 原文映射由本地维护，模型通过编号引用翻译片段；组装规则不依赖识别服务或语言对。 */
const scripts = [
  /\p{Script=Latin}/u, /\p{Script=Cyrillic}/u, /\p{Script=Han}/u,
  /\p{Script=Hiragana}/u, /\p{Script=Katakana}/u, /\p{Script=Hangul}/u,
];

/** 按词与书写系统切分引用单元，保留所有字符；空白附在前一个单元，避免独立空白编号。 */
export function referenceTranscript(text) {
  const units = [];
  let previous = null;
  for (const character of text) {
    if (/\s/u.test(character) && units.length > 0) {
      units[units.length - 1].text += character;
      previous = null;
      continue;
    }
    // 组合音标属于前一个词；其余按 Unicode 书写系统分组，不承担语言识别。
    const kind = /\p{Mark}/u.test(character) && previous !== null ? previous
      : scripts.findIndex((script) => script.test(character));
    if (previous === kind && units.length > 0) units[units.length - 1].text += character;
    else units.push({ id: units.length + 1, text: character });
    previous = kind;
  }
  return units;
}

/** 模型生成完整句子并决定输出顺序；原文映射只校验引用完整、无重复，不用原文覆盖模型的语法调整。 */
export function assembleTranslation(units, reply) {
  const start = reply.indexOf("{");
  const end = reply.lastIndexOf("}");
  if (start < 0 || end <= start) throw new Error("Translator returned no JSON object");
  const segments = JSON.parse(reply.slice(start, end + 1)).segments;
  if (!Array.isArray(segments) || segments.length === 0) throw new Error("Translator returned no sentence");
  const used = new Set();
  const result = [];
  for (const segment of segments) {
    const { from, to, text } = segment;
    if (!Number.isInteger(from) || !Number.isInteger(to) || from < 1 || to < from || to > units.length
      || typeof text !== "string") {
      throw new Error("Translator returned an invalid sentence span");
    }
    for (let id = from; id <= to; id++) {
      if (used.has(id)) throw new Error("Translator duplicated an original span");
      used.add(id);
    }
    const source = units.slice(from - 1, to).map((unit) => unit.text).join("");
    // 整句重排可把某个范围的词义融入其他输出片段，不能要求每段都独立输出词语。
    // 标点或空片段保留模型的排版，但不伪造只有标点的生词块。
    if (!lexicalText(source) || !lexicalText(text)) {
      appendText(result, text);
      continue;
    }
    appendChange(result, source, text);
  }
  if (units.some((unit) => lexicalText(unit.text) && !used.has(unit.id))) {
    throw new Error("Translator omitted original words");
  }
  if (!result.some((segment) => lexicalText(segment.text))) throw new Error("Translator returned no sentence words");
  return result;
}

/** 高亮从真实改写推导，不依赖模型的语言标签；纯排版变化不生成生词。 */
function lexicalText(text) {
  return text.replace(/[\s\p{P}\p{S}]/gu, "").toLowerCase();
}

function appendChange(result, source, text) {
  if (lexicalText(source) === lexicalText(text)) return appendText(result, text);
  const original = referenceTranscript(source).map((unit) => unit.text);
  const rewritten = referenceTranscript(text).map((unit) => unit.text);
  let first = 0, tail = 0;
  const same = (left, right) => lexicalText(left) === lexicalText(right);
  while (first < Math.min(original.length, rewritten.length) && same(original[first], rewritten[first])) first++;
  while (tail < Math.min(original.length, rewritten.length) - first
    && same(original[original.length - tail - 1], rewritten[rewritten.length - tail - 1])) tail++;
  const middle = (parts) => parts.slice(first, parts.length - tail).join("");
  // 插入或删除冠词等情况保留一个相邻词，确保词对两侧都有可学习的完整表达。
  while ((!lexicalText(middle(original)) || !lexicalText(middle(rewritten))) && (first > 0 || tail > 0)) {
    if (tail > 0) tail--;
    else first--;
  }
  const changed = middle(rewritten);
  appendText(result, rewritten.slice(0, first).join("") + changed.match(/^\s*/)[0]);
  result.push({ text: changed.trim(), source: middle(original).trim() });
  appendText(result, changed.match(/\s*$/)[0] + rewritten.slice(rewritten.length - tail).join(""));
}

function appendText(result, text) {
  if (!text) return;
  const last = result[result.length - 1];
  if (last && last.source === undefined) last.text += text;
  else result.push({ text });
}
