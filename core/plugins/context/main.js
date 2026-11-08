// 应用背景：默认行是通用背景（身份、常用词），应用行是该应用的补充；切换应用时两者按此顺序拼接为背景。
const DEFAULT_ROW = "default";

export async function focus(event) {
  const rows = await sayso.store.list("contexts");
  const base = rows.find((row) => row.key === DEFAULT_ROW);
  const app = event.app?.id && rows.find((row) => row.appId === event.app.id);
  const texts = [base, app].map((row) => row?.text.trim()).filter(Boolean);
  sayso.context.set(texts.join("\n"));
}
