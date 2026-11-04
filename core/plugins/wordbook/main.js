// 内置扩展「生词本」：在每张词条卡片上附加「加入生词本」按钮，词条多于一个时在整句卡片上附加「全部加入生词本」；
// 拥有「生词本」页签。卡片由翻译扩展与 Sayso 画出，本扩展只提供按钮。它与第三方扩展使用同一套 API，也是编写扩展的参考实现。

const COLLECTION = "words";
const chinese = sayso.languages.native.code === "zh";

/** 不自动记录：每个翻译或语法修正片段都提供加入入口，不因长度静默丢掉句子。 */
export function translated(event) {
  const entries = unique(event.pairs.filter((pair) => pair.card).map((pair) => ({
    card: pair.card, term: pair.target, gloss: pair.native, example: event.context ?? null, language: event.languages.target,
  })));
  // 每个按钮都带上本次全部词条与整句卡片，点击后据此判断是否已全部加入。
  const offer = (add, label, card) =>
    sayso.blocks.attach(card, [{ type: "invoke", label, payload: { add, all: entries, card: event.card } }]);
  for (const entry of entries) offer([entry], chinese ? "加入生词本" : "Add", entry.card);
  if (entries.length > 1 && event.card) offer(entries, chinese ? "全部加入生词本" : "Add all to wordbook", event.card);
}

/** 按钮载荷中带着要加入的词条，扩展本身不保存候选状态。 */
export async function action(event) {
  const { add, all, card } = event.action.payload;
  if (!event.surface) return;
  const surface = event.surface.id;

  // 同一候选框中每个词条只计一次：先单独加入、再「全部加入」时不重复计数。
  const before = await sayso.store.list(COLLECTION);
  for (const entry of add) {
    if (before.some((record) => record.key === key(entry) && record.surface === surface)) continue;
    const set = { term: entry.term, gloss: entry.gloss, language: entry.language, surface };
    // 划词没有上下文，不覆盖已有例句。
    if (entry.example) set.example = entry.example;
    sayso.store.patch(COLLECTION, key(entry), { set, increment: { count: 1 } });
  }

  // 已加入的词条把按钮换成不可点击的累计次数。
  const records = await sayso.store.list(COLLECTION);
  const saved = (entry) => records.find((record) => record.key === key(entry) && record.surface === surface);
  for (const entry of add) {
    const count = saved(entry)?.count ?? 1;
    done(entry.card, chinese ? `已加入生词本（第 ${count} 次）` : `Saved (×${count})`);
  }
  // 本次的词条都已加入时，整句卡片上的「全部加入」换成完成状态。
  if (all.length > 1 && card && all.every(saved)) done(card, chinese ? "已全部加入生词本" : "All saved to wordbook");
}

function done(card, label) {
  sayso.blocks.attach(card, [{ type: "invoke", label, disabled: true }]);
}

/** 同一张卡片只提供一次入口：同一词条的词对共用卡片。 */
function unique(entries) {
  return entries.filter((entry, index) => entries.findIndex((other) => other.card === entry.card) === index);
}

/** 去重键：「语言|词条」，忽略大小写、首尾空白与标点。 */
function key(entry) {
  const normalized = entry.term.replace(/^[\s\p{P}]+|[\s\p{P}]+$/gu, "").toLowerCase();
  return `${entry.language}|${normalized}`;
}
