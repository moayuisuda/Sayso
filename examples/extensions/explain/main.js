// 示例扩展「语法讲解」：演示订阅 `translated` hook、在整句卡片上附加按钮、按钮回调与 `sayso.llm`。
// 翻译完毕在整句卡片上附加「讲解语法」按钮；点击后按钮变为「讲解中…」，再用用户配置的模型讲解，扩展不需要 API Key。

const chinese = sayso.languages.native.code === "zh";
const BLOCK = "explain";

/** 讲解目标语言的句子：混说翻译为写入的整句，互译为词对的目标语言一侧；第三种语言没有可讲解的句子。 */
export function translated(event) {
  const sentence = event.context ?? event.pairs[0]?.target;
  if (!sentence || !event.card) return;
  sayso.blocks.attach(event.card, [{ type: "invoke", label: chinese ? "讲解语法" : "Explain grammar", payload: { sentence } }]);
}

/** 点击附加的按钮时，`blockId` 是整句卡片的引用；讲解结果显示为本扩展自己的卡片。 */
export async function action(event) {
  const { sentence } = event.action.payload;
  const card = event.action.blockId;
  sayso.blocks.attach(card, [{ type: "invoke", label: chinese ? "讲解中…" : "Explaining…", disabled: true }]);
  const explanation = await sayso.llm(
    "You are a friendly language teacher. Explain the grammar of the sentence the learner gave: "
      + "its structure, tense, and any idiom worth noticing. "
      + `Answer in ${sayso.languages.native.name}, in at most five short bullet points, as plain text.`,
    sentence,
  );
  sayso.blocks.upsert({ id: BLOCK, type: "card", title: sentence, body: explanation });
  sayso.blocks.attach(card, [{ type: "invoke", label: chinese ? "已讲解" : "Explained", disabled: true }]);
}
