// 示例扩展「Obsidian 记录」：把 Sayso 每次写入前台应用的文字追加到 Obsidian 库中当天的笔记。
// 演示订阅 `inserted` 与 `sayso.fs`；直接写库中的 Markdown 文件，Obsidian 未打开时也能记录。
//
// 配置见同目录 config.json：
// - vault：Obsidian 库的根目录，必须已存在（防止路径写错时在别处建出目录）。
// - notePath：库内笔记路径，{date} 替换为当天日期 YYYY-MM-DD。
//
// 每条记录一行，附写入的应用：
//   - 14:32 I have an apple. · Notes

const chinese = sayso.languages.native.code === "zh";

export function inserted(event) {
  const text = event.text.replace(/\s+/g, " ").trim();
  if (!text) return;

  const config = readConfig();
  if (!sayso.fs.exists(config.vault)) throw new Error(`Obsidian vault not found: ${config.vault}`);

  const now = new Date();
  const notePath = config.notePath.replace("{date}", `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`);
  const note = `${config.vault}/${notePath}`;
  sayso.fs.mkdir(note.slice(0, note.lastIndexOf("/")));
  const app = event.destination?.name ? ` · ${event.destination.name}` : "";
  sayso.fs.appendText(note, `- ${pad(now.getHours())}:${pad(now.getMinutes())} ${text}${app}\n`);

  sayso.blocks.upsert({
    id: "saved",
    type: "status",
    message: chinese ? `已记录到 Obsidian · ${notePath}` : `Saved to Obsidian · ${notePath}`,
    icon: "square.and.pencil",
    tone: "success",
  });
}

/** 相对路径相对扩展目录，`~` 展开为主目录。 */
function readConfig() {
  const config = JSON.parse(sayso.fs.readText("config.json"));
  if (!config.vault || !config.notePath) throw new Error('config.json needs "vault" and "notePath"');
  return config;
}

function pad(number) {
  return String(number).padStart(2, "0");
}
