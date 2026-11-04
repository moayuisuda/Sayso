// 示例扩展「收藏夹」：演示订阅广播事件、回调操作、扩展数据与页签。
//
// - translated：混说翻译、互译翻译完毕时广播的 hook，在每个短词的词条卡片上附加「收藏」按钮。
// - action：用户点了「收藏」，写入 favorites 集合，把按钮换成不可点击的「已收藏」。
//   收藏的词由 Sayso 按 extension.json 中的 tab 模板显示在「收藏」页签，无需本脚本参与。

const chinese = sayso.languages.native.code === "zh";
/** 超过这个词数的视为整句，不提供收藏。 */
const MAX_WORDS = 4;

/** 词条卡片由 Sayso 画出，`pair.card` 是卡片引用；同一词条的词对共用卡片，重复附加只会替换。 */
export function translated(event) {
  const language = event.languages.target;
  for (const { target: word, native: meaning, card } of event.pairs) {
    if (!card || word.split(/\s+/).length > MAX_WORDS) continue;
    sayso.blocks.attach(card, [{ type: "invoke", label: chinese ? "⭐ 收藏" : "⭐ Favorite", payload: { word, meaning, language } }]);
  }
}

/** 点击附加的按钮时，`blockId` 是被附加的卡片引用。 */
export function action(event) {
  const { word, meaning, language } = event.action.payload;
  sayso.store.patch("favorites", key(language, word), { set: { word, meaning, language }, increment: { count: 1 } });
  sayso.blocks.attach(event.action.blockId, [{ type: "invoke", label: chinese ? "⭐ 已收藏" : "⭐ Saved", disabled: true }]);
}

function key(language, word) {
  return `${language}|${word.toLowerCase()}`;
}
