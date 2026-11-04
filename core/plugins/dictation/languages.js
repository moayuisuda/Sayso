/** 混说翻译的语言资料：每种语言只维护自身特点和等义例句，不复制语言对模板。 */
export const languageProfiles = {
  en: {
    source: "Recognize English phrases by their syntax and meaning, including contractions; Latin letters alone do not identify English.",
    target: "Use natural English articles, countability, agreement and prepositions. Preserve the speaker's own English wording unless an adjacent translation requires a grammatical change.",
    greeting: "Hello.", acknowledgement: "I understand.", plan: "I will go to the zoo.",
  },
  zh: {
    source: "Identify Chinese phrases from vocabulary and grammar, including short greetings. Han characters alone do not establish that a phrase is Chinese.",
    target: "Use natural Chinese word order and measure words. Preserve the speaker's own Chinese wording and do not add spaces between Chinese words.",
    greeting: "你好。", acknowledgement: "明白了。", plan: "我会去动物园。",
  },
  ja: {
    source: "Identify Japanese from vocabulary, particles and conjugation. Japanese can contain kana, kanji or both; do not classify a kanji-only phrase by its script alone.",
    target: "Use natural Japanese particles, conjugation and word order. Match the speaker's existing politeness level, and retain normal Japanese kanji and kana spelling.",
    greeting: "こんにちは。", acknowledgement: "わかりました。", plan: "動物園に行きます。",
  },
  ko: {
    source: "Identify Korean phrases from vocabulary and grammar, including particles, endings and short expressions; script alone is not a complete language judgement.",
    target: "Use natural Korean particles, spacing and conjugation. Match the speaker's existing speech level and honorifics.",
    greeting: "안녕하세요.", acknowledgement: "알겠습니다.", plan: "동물원에 갈 거예요.",
  },
  fr: {
    source: "Identify French using vocabulary, syntax and elision. Accents may be absent in a transcript; shared Latin spelling does not make a phrase another language.",
    target: "Use natural French articles, elision, gender and number agreement, and prepositions. Change neighbouring French words only when the translation requires agreement.",
    greeting: "Bonjour.", acknowledgement: "J'ai compris.", plan: "J'irai au zoo.",
  },
  de: {
    source: "Identify German from vocabulary and syntax, including compounds; Latin spelling and shared words alone do not determine the language.",
    target: "Use natural German case, article and adjective agreement, noun capitalization and verb placement. Preserve the speaker's existing register.",
    greeting: "Hallo.", acknowledgement: "Verstanden.", plan: "Ich werde in den Zoo gehen.",
  },
  es: {
    source: "Identify Spanish using vocabulary and syntax even when accents are missing. Shared Latin words alone do not establish the language.",
    target: "Use natural Spanish gender and number agreement, conjugation and prepositions. Retain the speaker's existing form of address and regional wording.",
    greeting: "Hola.", acknowledgement: "Entendido.", plan: "Iré al zoológico.",
  },
  ru: {
    source: "Identify Russian from vocabulary and grammar, including inflected words; Cyrillic characters alone do not establish that a phrase is Russian.",
    target: "Use natural Russian case, agreement, verb aspect and prepositions. Preserve the speaker's existing register and unchanged Russian wording.",
    greeting: "Здравствуйте.", acknowledgement: "Понятно.", plan: "Я пойду в зоопарк.",
  },
};

/** 仅保存无法归属于单种语言的交互规则，语言对无方向时使用排序后的键。 */
export const languagePairRules = {
  "ja:zh": "Chinese and Japanese share Han characters. Distinguish them by vocabulary and grammar: Chinese 你好 and Japanese こんにちは are different-language greetings. Translate every native-language phrase into the target language, while retaining legitimate target-language Han characters.",
};
