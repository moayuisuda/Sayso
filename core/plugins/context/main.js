// 应用背景：切换应用时，用该应用这一行的文本作为背景；没有对应行或文本为空时用默认行。
const DEFAULT_ROW = "default";

export async function focus(event) {
  const rows = await sayso.store.list("contexts");
  const appRow = rows.find((row) => row.appId && row.appId === event.app?.id && row.text.trim());
  const row = appRow ?? rows.find((row) => row.key === DEFAULT_ROW);
  sayso.context.set(row?.text ?? "");
}
