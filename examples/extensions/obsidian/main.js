// 示例扩展「Obsidian 记录」：在整句卡片与每张词条卡片上附加「记到 Obsidian」按钮，点击后写成一张卡片，
// 追加到 Obsidian 库中当天的笔记。演示订阅 `translated` hook、`sayso.blocks.attach`、按钮回调 `action` 与 `sayso.fs`；
// 直接写库中的 Markdown 文件，Obsidian 未打开时也能记录。
//
// 配置见同目录 config.json：
// - vault：Obsidian 库的根目录，必须已存在（防止路径写错时在别处建出目录）。
// - notePath：库内笔记路径，{date} 替换为当天日期 YYYY-MM-DD。
//
// 每次记录是一个 Obsidian callout 卡片，标题为时间与触发时的应用。
// 整句卡片上的按钮记下整次翻译：混说翻译为写入的整句（翻译的片段高亮）与各片段的「译文 · 原文」，互译为原文与译文；
//   > [!note] 14:32 · Notes
//   > I have ==an apple== and ==bananas==.
//   >
//   > - ==an apple== · a 苹果
//   > - ==bananas== · 香蕉
// 词条卡片上的按钮只记这个词条，有整句时附上整句并高亮该词条：
//   > [!note] 14:33 · Notes
//   > ==an apple== · a 苹果
//   >
//   > I have ==an apple== and bananas.

const chinese = sayso.languages.native.code === "zh";
const label = chinese ? "记到 Obsidian" : "Save to Obsidian";

/** 按钮载荷中带着整理好的卡片，点击时不再依赖本次事件；同一词条的词对共用卡片，重复附加只会替换。 */
export function translated(event) {
  const app = event.source?.name ?? null;
  if (event.card) {
    sayso.blocks.attach(event.card, [{ type: "invoke", label, payload: { body: translationBody(event), app } }]);
  }
  for (const pair of event.pairs) {
    if (!pair.card) continue;
    sayso.blocks.attach(pair.card, [{ type: "invoke", label, payload: { body: pairBody(pair, event.context), app } }]);
  }
}

/** 点击附加的按钮时，`blockId` 是被附加的卡片引用。 */
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

  // 换成不可点击的完成状态，避免重复记录。
  sayso.blocks.attach(event.action.blockId, [{ type: "invoke", label: chinese ? "已记到 Obsidian" : "Saved to Obsidian", disabled: true }]);
}

/** 整次翻译的正文：混说翻译为高亮后的整句与词对列表，互译为原文与译文。 */
function translationBody(event) {
  if (event.input.via === "selection") return [event.input.text, event.output];
  const pairs = event.pairs.map((pair) => `- ==${pair.target}== · ${pair.native}`).join("\n");
  return pairs ? [highlightAll(event.output, event.pairs), pairs] : [event.output];
}

/** 单个词条的正文：词对；有整句上下文时附上整句并高亮该词条。 */
function pairBody(pair, context) {
  const entry = `==${pair.target}== · ${pair.native}`;
  return context ? [entry, highlightAll(context, [pair])] : [entry];
}

/** 按顺序在整句中找到每个译文片段并高亮；找不到的片段只出现在词对中。 */
function highlightAll(sentence, pairs) {
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
