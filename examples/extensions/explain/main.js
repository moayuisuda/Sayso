// 示例扩展「语法讲解」：演示命令与 `sayso.llm`。
// 把命令绑到选区路由（如 ⌥T）讲解选中的句子，绑到语音路由则讲解说出的句子；扩展本身拿到的都是 `input.text`。
// 先显示「讲解中…」，再用用户配置的模型讲解，扩展不需要 API Key。

const chinese = sayso.languages.native.code === "zh";

export async function command(event) {
  if (event.command.id !== "explain") return;
  const sentence = event.input.text;
  sayso.blocks.upsert({ id: "explain", type: "status", message: chinese ? "讲解中…" : "Explaining…", icon: "sparkles" });
  const explanation = await sayso.llm(
    "You are a friendly language teacher. Explain the grammar of the sentence the learner gave: "
      + "its structure, tense, and any idiom worth noticing. "
      + `Answer in ${sayso.languages.native.name}, in at most five short bullet points, as plain text.`,
    sentence,
  );
  sayso.blocks.upsert({ id: "explain", type: "card", title: sentence, body: explanation });
}
