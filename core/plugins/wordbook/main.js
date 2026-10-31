// 内置扩展「生词本」：在候选框中列出翻译出的单词、短语，每个词条可单独加入生词本，也可一键全部加入；
// 拥有「生词本」页签。它与第三方扩展使用同一套 API，也是编写扩展的参考实现。

const ID = "sayso.wordbook";
const COLLECTION = "words";
const ADD_ALL = "add-all";
const chinese = sayso.languages.native.code === "zh";

/** 不自动记录：把词对逐个列为带「加入生词本」按钮的词条。整句、整段翻译不是生词。 */
export function translation(event) {
  const entries = event.pairs
    .filter((pair) => isWordOrPhrase(pair.target))
    .map((pair) => ({ term: pair.target, gloss: pair.native, example: event.context ?? null, language: event.languages.target }));
  offer(entries);
}

/** 按钮载荷中带着要加入的词条，扩展本身不保存候选状态。 */
export async function action(event) {
  const requested = event.action.payload;
  if (!Array.isArray(requested)) return;
  // 「全部加入」时跳过已单独加入过的词条，避免重复计数。
  const pending = pendingKeys(event.surface);
  const toSave = requested.filter((entry) => pending === null || pending.has(key(entry)));
  if (toSave.length > 0) await save(toSave, pending);
}

function offer(entries) {
  const unique = [];
  for (const entry of entries) {
    if (!unique.some((item) => key(item) === key(entry))) unique.push(entry);
  }
  for (const entry of unique) {
    sayso.blocks.upsert({
      id: key(entry),
      type: "term",
      term: entry.term,
      gloss: entry.gloss,
      language: entry.language,
      actions: [addAction([entry], chinese ? "加入生词本" : "Add")],
    });
  }
  if (unique.length > 1) {
    sayso.blocks.upsert({
      id: ADD_ALL,
      type: "status",
      message: chinese ? `本次 ${unique.length} 个生词` : `${unique.length} new words`,
      icon: "book",
      tone: "info",
      actions: [addAction(unique, chinese ? "全部加入生词本" : "Add all to wordbook")],
    });
  }
}

function addAction(entries, label) {
  return { type: "invoke", label, payload: entries };
}

async function save(entries, pending) {
  for (const entry of entries) {
    const set = { term: entry.term, gloss: entry.gloss, language: entry.language };
    // 划词没有上下文，不覆盖已有例句。
    if (entry.example) set.example = entry.example;
    sayso.store.patch(COLLECTION, key(entry), { set, increment: { count: 1 } });
  }

  // 已加入的词条去掉按钮并显示累计次数；全部加入后「全部加入」按钮换成完成提示。
  const records = await sayso.store.list(COLLECTION);
  for (const entry of entries) {
    const count = records.find((record) => record.key === key(entry))?.count ?? 1;
    sayso.blocks.upsert({
      id: key(entry),
      type: "term",
      term: entry.term,
      gloss: entry.gloss,
      language: entry.language,
      detail: chinese ? `已加入生词本（第 ${count} 次）` : `Saved to wordbook (×${count})`,
    });
  }
  const saved = new Set(entries.map(key));
  if (pending !== null && [...pending].every((item) => saved.has(item))) {
    sayso.blocks.upsert({
      id: ADD_ALL,
      type: "status",
      message: chinese ? "已全部加入生词本" : "All saved to wordbook",
      icon: "book.fill",
      tone: "success",
    });
  }
}

/** 候选框中仍带「加入」按钮的本扩展词条；没有候选框时为 null。 */
function pendingKeys(surface) {
  if (!surface) return null;
  const prefix = `${ID}/`;
  const keys = surface.blocks
    .filter((block) => block.id.startsWith(prefix) && (block.actions ?? []).length > 0)
    .map((block) => block.id.slice(prefix.length))
    .filter((item) => item !== ADD_ALL);
  return new Set(keys);
}

/** 去重键：「语言|词条」，忽略大小写、首尾空白与标点。 */
function key(entry) {
  const normalized = entry.term.replace(/^[\s\p{P}]+|[\s\p{P}]+$/gu, "").toLowerCase();
  return `${entry.language}|${normalized}`;
}

/** 单词、短语或习语：不超过 6 个词且不长于 40 个字符。 */
function isWordOrPhrase(text) {
  return text.trim().split(/\s+/).filter(Boolean).length <= 6 && [...text].length <= 40;
}
