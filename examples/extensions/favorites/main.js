// 示例扩展「收藏夹」：演示订阅广播事件、回调操作、扩展数据与页签。
//
// - translated：语音翻译、划词翻译翻译完毕时广播的 hook，为每个短词显示一张带「收藏」按钮的卡片。
// - action：用户点了「收藏」，写入 favorites 集合并显示状态块。
//   收藏的词由 Sayso 按 extension.json 中的 tab 模板显示在「收藏」页签，无需本脚本参与。

const chinese = sayso.languages.native.code === "zh";
/** 超过这个词数的视为整句，不提供收藏。 */
const MAX_WORDS = 4;

export function translated(event) {
  const language = event.languages.target;
  for (const { target: word, native: meaning } of event.pairs) {
    if (word.split(/\s+/).length > MAX_WORDS) continue;
    sayso.blocks.upsert({
      id: `favorite-${key(language, word)}`,
      type: "card",
      title: word,
      body: meaning,
      actions: [{ type: "invoke", label: chinese ? "⭐ 收藏" : "⭐ Favorite", payload: { word, meaning, language } }],
    });
  }
}

export function action(event) {
  const { word, meaning, language } = event.action.payload;
  sayso.store.patch("favorites", key(language, word), { set: { word, meaning, language }, increment: { count: 1 } });
  sayso.blocks.upsert({
    id: event.action.blockId,
    type: "status",
    message: chinese ? `已收藏「${word}」` : `Saved “${word}” to favorites`,
    icon: "star.fill",
    tone: "success",
  });
}

function key(language, word) {
  return `${language}|${word.toLowerCase()}`;
}
