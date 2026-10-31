// 示例扩展「待办」：演示命令与扩展数据。
//
// - command：把输入写入 todos 集合，并在候选框显示「已添加」。输入来源由路由决定：
//   路由设为语音时记下说的话；设为选区时记下选中的文字。路由设为「直接」则与「语音翻译」同时触发：
//   说一句话，既翻译写入，也记成待办。
//   待办由 Sayso 按 extension.json 中的 tab 模板显示在「待办」页签，完成时点「完成」删除，无需本脚本参与。

const chinese = sayso.languages.native.code === "zh";

export function command(event) {
  if (event.command.id !== "add") return;
  const text = event.input.text;
  // 事件 id 唯一，直接作为记录 key。
  sayso.store.put("todos", event.id, { text, app: event.source?.name ?? "" });
  sayso.blocks.upsert({
    id: "added",
    type: "status",
    message: chinese ? `已添加待办「${text}」` : `Added “${text}”`,
    icon: "checklist",
    tone: "success",
  });
}
