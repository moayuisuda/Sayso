// 示例扩展「Obsidian 记录」：翻译完毕后在候选框中显示「记到 Obsidian」按钮，点击后把这次翻译写成一张卡片，
// 追加到 Obsidian 库中当天的笔记。演示订阅 `translated` hook、按钮回调 `action` 与 `sayso.fs`；
// 直接写库中的 Markdown 文件，Obsidian 未打开时也能记录。
//
// 配置见同目录 config.json：
// - vault：Obsidian 库的根目录，必须已存在（防止路径写错时在别处建出目录）。
// - notePath：库内笔记路径，{date} 替换为当天日期 YYYY-MM-DD。
//
// 每次记录是一个 Obsidian callout 卡片，标题为时间与触发时的应用：
// 语音翻译记下写入的整句，其中实际翻译的片段高亮，下方列出各片段的「译文 · 原文」；
//   > [!note] 14:32 · Notes
//   > I have ==an apple== and ==bananas==.
//   >
//   > - ==an apple== · a 苹果
//   > - ==bananas== · 香蕉
// 划词翻译记下原文，下方是译文。

const chinese = sayso.languages.native.code === "zh";
const BLOCK = "save";

/** 按钮载荷中带着整理好的卡片，点击时不再依赖本次事件。 */
export function translated(event) {
  sayso.blocks.upsert({
    id: BLOCK,
    type: "status",
    message: chinese ? "记到 Obsidian 当天的笔记" : "Save to today's Obsidian note",
    icon: "square.and.pencil",
    actions: [{
      type: "invoke",
      label: chinese ? "记到 Obsidian" : "Save to Obsidian",
      payload: { body: cardBody(event), app: event.source?.name ?? null },
    }],
  });
}

export function action(event) {
  const { body, app } = event.action.payload;
  const config = readConfig();
  if (!sayso.fs.exists(config.vault)) throw new Error(`Obsidian vault not found: ${config.vault}`);

  const now = new Date();
  const notePath = config.notePath.replace("{date}", `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`);
  const note = `${config.vault}/${notePath}`;
  sayso.fs.mkdir(note.slice(0, note.lastIndexOf("/")));
  const title = `${pad(now.getHours())}:${pad(now.getMinutes())}${app ? ` · ${app}` : ""}`;
  // 卡片前留空行，与上一条记录或笔记中的其他内容分开，callout 才能独立渲染。
  sayso.fs.appendText(note, `\n${callout(title, body)}\n`);

  // 同 id 替换，去掉按钮，避免重复记录。
  sayso.blocks.upsert({
    id: BLOCK,
    type: "status",
    message: chinese ? `已记录到 Obsidian · ${notePath}` : `Saved to Obsidian · ${notePath}`,
    icon: "square.and.pencil",
    tone: "success",
  });
}

/** 卡片正文的各段：语音翻译为高亮后的整句与词对列表，划词翻译为原文与译文。 */
function cardBody(event) {
  if (event.input.via === "selection") return [event.input.text, event.output];
  const pairs = event.pairs.map((pair) => `- ==${pair.target}== · ${pair.native}`).join("\n");
  return pairs ? [highlight(event.output, event.pairs), pairs] : [event.output];
}

/** 按顺序在整句中找到每个译文片段并高亮；找不到的片段只出现在下方列表中。 */
function highlight(sentence, pairs) {
  let result = "";
  let cursor = 0;
  for (const { target } of pairs) {
    const index = sentence.indexOf(target, cursor);
    if (index < 0) continue;
    result += `${sentence.slice(cursor, index)}==${target}==`;
    cursor = index + target.length;
  }
  return result + sentence.slice(cursor);
}

/** 每段之间空一行，所有行加上 `> ` 前缀，多行原文也留在卡片内。 */
function callout(title, paragraphs) {
  const lines = paragraphs.join("\n\n").split("\n").map((line) => (line ? `> ${line}` : ">"));
  return [`> [!note] ${title}`, ...lines].join("\n");
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
