// 示例扩展「语法讲解」：演示订阅 `translated` hook、按钮回调与 `sayso.llm`。
// 翻译完毕后显示「讲解语法」按钮；点击后先显示「讲解中…」，再用用户配置的模型讲解，扩展不需要 API Key。

const chinese = sayso.languages.native.code === "zh";
const BLOCK = "explain";

/** 讲解目标语言的句子：语音翻译为写入的整句，划词翻译为词对的目标语言一侧；第三种语言没有可讲解的句子。 */
export function translated(event) {
  const sentence = event.context ?? event.pairs[0]?.target;
  if (!sentence) return;
  sayso.blocks.upsert({
    id: BLOCK,
    type: "status",
    message: chinese ? "讲解这句的语法" : "Explain the grammar of this sentence",
    icon: "sparkles",
    actions: [{ type: "invoke", label: chinese ? "讲解语法" : "Explain grammar", payload: { sentence } }],
  });
}

export async function action(event) {
  const { sentence } = event.action.payload;
  sayso.blocks.upsert({ id: BLOCK, type: "status", message: chinese ? "讲解中…" : "Explaining…", icon: "sparkles" });
  const explanation = await sayso.llm(
    "You are a friendly language teacher. Explain the grammar of the sentence the learner gave: "
      + "its structure, tense, and any idiom worth noticing. "
      + `Answer in ${sayso.languages.native.name}, in at most five short bullet points, as plain text.`,
    sentence,
  );
  sayso.blocks.upsert({ id: BLOCK, type: "card", title: sentence, body: explanation });
}
