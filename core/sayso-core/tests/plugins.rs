//! 内置插件与插件运行时的行为测试：用伪造的宿主固定模型回复，断言插件输出的指令、写入与投递。

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sayso_core::error::CoreError;
use sayso_core::plugin::{built_in_plugins, load_installed, run, Host, Instruction, Plugin, RunResult, RunStatus};
use sayso_core::protocol::{
    ActionInvocation, ActionKind, ActionOrigin, AppInfo, Block, BlockContent, CommandRef, Delivery, Event, Input,
    InputSource, Language, LanguagePair, Payload, SurfaceSnapshot, TextStyle, Translation, TranslationPair,
};
use sayso_core::store::{StoreRecord, Stores};

/// 记录插件输出的宿主替身：模型固定回复，数据写入临时目录，其余指令按顺序记下。
struct FakeHost {
    extension_id: String,
    reply: String,
    delivery: Delivery,
    insert_error: Mutex<Option<String>>,
    stores: Arc<Stores>,
    instructions: Mutex<Vec<Instruction>>,
    inserted: Mutex<Vec<String>>,
    completions: AtomicUsize,
    prompts: Mutex<Vec<String>>,
}

impl FakeHost {
    fn new(extension_id: &str, reply: &str, delivery: Delivery, stores: Arc<Stores>) -> Arc<Self> {
        Arc::new(FakeHost {
            extension_id: extension_id.into(),
            reply: reply.into(),
            delivery,
            insert_error: Mutex::default(),
            stores,
            instructions: Mutex::default(),
            inserted: Mutex::default(),
            completions: AtomicUsize::new(0),
            prompts: Mutex::default(),
        })
    }

    fn instructions(&self) -> Vec<Instruction> {
        self.instructions.lock().unwrap().clone()
    }

    fn blocks(&self) -> Vec<Block> {
        self.instructions()
            .into_iter()
            .filter_map(|instruction| match instruction {
                Instruction::UpsertBlock(block) => Some(block),
                _ => None,
            })
            .collect()
    }

    fn inserted(&self) -> Vec<String> {
        self.inserted.lock().unwrap().clone()
    }
}

#[async_trait]
impl Host for FakeHost {
    fn apply(&self, instruction: Instruction) -> Result<(), CoreError> {
        if let Instruction::Store(write) = instruction {
            return self.stores.apply(&self.extension_id, write);
        }
        self.instructions.lock().unwrap().push(instruction);
        Ok(())
    }

    async fn complete(&self, system: String, _user: String) -> Result<String, CoreError> {
        self.completions.fetch_add(1, Ordering::SeqCst);
        self.prompts.lock().unwrap().push(system);
        Ok(self.reply.clone())
    }

    async fn insert(&self, text: String) -> Result<Delivery, CoreError> {
        if let Some(message) = self.insert_error.lock().unwrap().clone() {
            return Err(CoreError::Platform(sayso_core::PlatformError::Failed { message }));
        }
        self.inserted.lock().unwrap().push(text);
        Ok(self.delivery)
    }

    fn records(&self, collection: &str) -> Result<Vec<StoreRecord>, CoreError> {
        self.stores.records(&self.extension_id, collection)
    }
}

fn built_in(id: &str) -> Plugin {
    built_in_plugins().into_iter().find(|plugin| plugin.id() == id).expect("built-in plugin exists")
}

fn temporary_stores() -> (tempfile::TempDir, Arc<Stores>) {
    let directory = tempfile::tempdir().unwrap();
    let stores = Arc::new(Stores::new(directory.path().to_path_buf()));
    (directory, stores)
}

fn command(text: &str, via: InputSource) -> Event {
    Event::new(
        LanguagePair::default(),
        Some(SurfaceSnapshot { id: "s".into(), blocks: vec![] }),
        Payload::Command {
            command: CommandRef { id: "translate".into() },
            input: Input { text: text.into(), via },
            source: Some(AppInfo { id: Some("com.apple.Notes".into()), name: Some("Notes".into()) }),
        },
    )
}

fn block_ids(blocks: &[Block]) -> Vec<&str> {
    blocks.iter().map(|block| block.id.as_str()).collect()
}

fn speaks(instructions: &[Instruction]) -> Vec<(String, Language)> {
    instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::Speak { text, language } => Some((text.clone(), *language)),
            _ => None,
        })
        .collect()
}

fn pair(target: &str, native: &str) -> TranslationPair {
    TranslationPair { target: target.into(), native: native.into() }
}

async fn run_dictation(transcript: &str, reply: &str, delivery: Delivery) -> (RunResult, Arc<FakeHost>) {
    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new("sayso.dictation", reply, delivery, stores);
    let result = run(&built_in("sayso.dictation"), &command(transcript, InputSource::Speech), host.clone()).await;
    (result, host)
}

/// 混说翻译后写入译文（译文可连带改写相邻词 a → an），输出文本块与剪贴板提示，朗读翻译块，最后广播词对。
#[tokio::test]
async fn dictation_translates_inserts_speaks_and_publishes() {
    let reply = r#"{"segments":[{"from":1,"to":2,"text":"I have "},{"from":3,"to":4,"text":"an apple"},{"from":5,"to":5,"text":"."}]}"#;
    let (result, host) = run_dictation("I have a 苹果.", reply, Delivery::CopiedToClipboard).await;

    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(host.inserted(), ["I have an apple."]);
    assert_eq!(block_ids(&host.blocks()), ["text", "clipboard"]);
    let BlockContent::Text { runs, .. } = &host.blocks()[0].content else { panic!("expected text block") };
    let highlighted: Vec<_> =
        runs.iter().filter(|run| run.style == Some(TextStyle::Highlight)).map(|run| &run.text).collect();
    assert_eq!(highlighted, ["an apple"]);
    assert_eq!(speaks(&host.instructions()), [("an apple".to_string(), Language::English)]);
    assert_eq!(
        host.instructions().last(),
        Some(&Instruction::PublishTranslation {
            pairs: vec![pair("an apple", "a 苹果")],
            context: Some("I have an apple.".into())
        })
    );
}

/// 写入平台报错时保留译文与生词事件，不得声称已复制到剪贴板。
#[tokio::test]
async fn dictation_preserves_translation_and_word_pairs_when_insertion_fails() {
    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new(
        "sayso.dictation",
        r#"{"segments":[{"from":1,"to":2,"text":"I have "},{"from":3,"to":4,"text":"an apple"},{"from":5,"to":5,"text":"."}]}"#,
        Delivery::Inserted,
        stores,
    );
    *host.insert_error.lock().unwrap() = Some("Clipboard snapshot failed".into());
    let result = run(&built_in("sayso.dictation"), &command("I have a 苹果.", InputSource::Speech), host.clone()).await;
    assert_eq!(result.status, RunStatus::Succeeded);
    assert!(host.inserted().is_empty());
    let blocks = host.blocks();
    assert_eq!(block_ids(&blocks), ["text", "insertion"]);
    let BlockContent::Status { message, .. } = &blocks[1].content else { panic!("expected insertion error") };
    assert!(message.contains("Clipboard snapshot failed"));
    assert!(!message.contains("copied to clipboard"));
    assert!(host.instructions().iter().any(|instruction| matches!(instruction,
        Instruction::PublishTranslation { pairs, .. } if pairs == &[pair("an apple", "a 苹果")]
    )));
}

/// 译文两侧的空白移到普通文本中，相邻普通文本合并，高亮片段只包含译文本身。
#[tokio::test]
async fn dictation_moves_whitespace_around_translations_into_text() {
    let reply = r#"{"segments":[{"from":1,"to":3,"text":"I want to "},{"from":4,"to":4,"text":"eat an apple "},{"from":5,"to":7,"text":"after lunch."}]}"#;
    let (_, host) = run_dictation("I want to 吃一个苹果 after lunch.", reply, Delivery::Inserted).await;

    let BlockContent::Text { runs, .. } = &host.blocks()[0].content else { panic!("expected text block") };
    let texts: Vec<_> = runs.iter().map(|run| run.text.as_str()).collect();
    assert_eq!(texts, ["I want to ", "eat an apple", " after lunch."]);
    assert_eq!(host.inserted(), ["I want to eat an apple after lunch."]);
}

/// 纯目标语言输入不调用模型，原样写入，也不广播词对。
#[tokio::test]
async fn dictation_skips_model_for_pure_target_language() {
    let (result, host) = run_dictation("  I want an apple today.  ", "", Delivery::Inserted).await;

    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(host.completions.load(Ordering::SeqCst), 0);
    assert_eq!(host.inserted(), ["I want an apple today."]);
    assert!(!host.instructions().iter().any(|i| matches!(i, Instruction::PublishTranslation { .. })));
}

/// 翻译引用了不存在的原文时，不提交不可信结果。
#[tokio::test]
async fn dictation_inserts_transcript_when_translation_is_untrusted() {
    let reply = r#"{"segments":[{"from":1,"to":2,"text":"I have "},{"from":3,"to":9,"text":"a banana"}]}"#;
    let (result, host) = run_dictation("I have a 苹果.", reply, Delivery::Inserted).await;

    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(host.inserted(), ["I have a 苹果."]);
    assert_eq!(block_ids(&host.blocks()), ["text", "notice"]);
    assert!(speaks(&host.instructions()).is_empty());
    assert!(!host.instructions().iter().any(|i| matches!(i, Instruction::PublishTranslation { .. })));
}

/// 所有支持的语言对都选择自身资料，且生成的完整分段示例能通过真实插件的校验与发布流程。
#[tokio::test]
async fn dictation_prompt_profiles_cover_all_language_pairs_without_leaking_pair_rules() {
    let plugin = built_in("sayso.dictation");
    for target in Language::ALL {
        for native in Language::ALL.into_iter().filter(|native| *native != target) {
            let (_directory, stores) = temporary_stores();
            let host = FakeHost::new("sayso.dictation", "", Delivery::Inserted, stores.clone());
            // 每种书写系统均有字符，确保所有语言对都会经过正式的模型调用入口。
            let mut event = command("a 中 あ 한 я", InputSource::Speech);
            event.languages = LanguagePair { target, native };
            let result = run(&plugin, &event, host.clone()).await;
            assert_eq!(result.status, RunStatus::Succeeded);
            let prompts = host.prompts.lock().unwrap();
            assert_eq!(prompts.len(), 1, "{target:?}/{native:?}");
            let prompt = &prompts[0];
            assert!(prompt.contains(&format!("Source language ({}):", native.english_name())));
            assert!(prompt.contains(&format!("Target language ({}):", target.english_name())));
            assert!(prompt.contains(&format!(
                "First-pass recognition repair ({} sounds written as {}):",
                target.english_name(),
                native.english_name()
            )));
            assert!(prompt.contains("recovery of misrecognized TARGET-LANGUAGE SPEECH, not semantic translation"));
            assert!(prompt.contains("prefer the normal native meaning unless context clearly establishes an ASR error"));
            assert!(prompt.contains("The original audio is not available to you"));
            let shares_han = matches!(
                (target, native),
                (Language::Japanese, Language::Chinese) | (Language::Chinese, Language::Japanese)
            );
            assert_eq!(prompt.contains("Language-pair guidance:"), shares_han, "{target:?}/{native:?}");
            assert_eq!(prompt.contains("Chinese and Japanese share Han"), shares_han);

            let example = prompt.lines().last().unwrap();
            let json: serde_json::Value = serde_json::from_str(example).unwrap();
            let segments = json["output"]["segments"].as_array().unwrap();
            let units = json["input"]["units"].as_array().unwrap();
            assert_eq!(segments.len(), 3);
            assert!(segments.iter().all(|segment| segment.get("language").is_none()));
            let source = |segment: &serde_json::Value| -> String {
                units[segment["from"].as_u64().unwrap() as usize - 1..segment["to"].as_u64().unwrap() as usize]
                    .iter()
                    .map(|unit| unit["text"].as_str().unwrap())
                    .collect()
            };
            assert_eq!(source(&segments[1]), segments[1]["text"].as_str().unwrap());
            let transcript = json["input"]["transcript"].as_str().unwrap();
            assert_eq!(units.iter().map(|unit| unit["text"].as_str().unwrap()).collect::<String>(), transcript);
            let output: String = segments.iter().map(|segment| segment["text"].as_str().unwrap()).collect();

            // 把生成的例句作为输入及模型回复，验证组装、完整覆盖校验与两个词对的发布能配合工作。
            let host = FakeHost::new("sayso.dictation", &json["output"].to_string(), Delivery::Inserted, stores);
            let mut event = command(transcript, InputSource::Speech);
            event.languages = LanguagePair { target, native };
            assert_eq!(run(&plugin, &event, host.clone()).await.status, RunStatus::Succeeded);
            assert_eq!(host.inserted(), [output], "{target:?}/{native:?}");
            let Some(Instruction::PublishTranslation { pairs, .. }) = host.instructions().last().cloned() else {
                panic!("expected translation pairs for {target:?}/{native:?}")
            };
            assert_eq!(pairs.len(), 2);
            assert!(source(&segments[0]).contains(&pairs[0].native), "{pairs:?}");
            assert!(source(&segments[2]).contains(&pairs[1].native), "{pairs:?}");
        }
    }
}

/// 日中共享汉字仍需逐段翻译；重复出现的中文、首尾中文均保留独立的词对。
#[tokio::test]
async fn dictation_covers_all_japanese_chinese_spans() {
    let (_directory, stores) = temporary_stores();
    let reply = r#"{"segments":[{"from":1,"to":2,"text":"こんにちは。"},{"from":3,"to":4,"text":"わかりました。"},{"from":5,"to":6,"text":"こんにちは。"},{"from":7,"to":8,"text":"動物園に行きます。"}]}"#;
    let host = FakeHost::new("sayso.dictation", reply, Delivery::Inserted, stores);
    let mut event = command("你好。わかりました。你好。我会去动物园。", InputSource::Speech);
    event.languages.target = Language::Japanese;
    let result = run(&built_in("sayso.dictation"), &event, host.clone()).await;
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(host.inserted(), ["こんにちは。わかりました。こんにちは。動物園に行きます。"]);
    let Some(Instruction::PublishTranslation { pairs, .. }) = host.instructions().last().cloned() else {
        panic!("expected translation pairs")
    };
    assert_eq!(
        pairs,
        [pair("こんにちは", "你好"), pair("こんにちは", "你好"), pair("動物園に行きます", "我会去动物园")]
    );
}

/// 漏掉引用、重复引用、无效范围与空译文均不能作为完整翻译提交。
#[tokio::test]
async fn dictation_rejects_incomplete_or_misannotated_translations() {
    for reply in [
        r#"{"segments":[{"from":1,"to":2,"text":"apples and "}]}"#,
        r#"{"segments":[{"from":1,"to":3,"text":"apples and bananas"},{"from":3,"to":3,"text":"bananas"}]}"#,
        r#"{"segments":[{"from":0,"to":3,"text":"apples and bananas"}]}"#,
        r#"{"segments":[{"from":1,"to":4,"text":"apples and bananas"}]}"#,
        r#"{"segments":[{"from":1.5,"to":3,"text":"apples and bananas"}]}"#,
        r#"{"segments":[{"from":1,"to":3,"text":" "}]}"#,
    ] {
        let (_, host) = run_dictation("苹果 和 香蕉", reply, Delivery::Inserted).await;
        assert_eq!(host.inserted(), ["苹果 和 香蕉"]);
        assert_eq!(block_ids(&host.blocks()), ["text", "notice"]);
        assert!(!host
            .instructions()
            .iter()
            .any(|instruction| matches!(instruction, Instruction::PublishTranslation { .. })));
    }
}

/// 截图中的混说句可调整完整输出的标点、大小写；中文原文直接取本地引用，模型不再重抄 source。
#[tokio::test]
async fn dictation_regression_preserves_source_mapping_while_composing_the_whole_sentence() {
    let transcript = "You'd better 统一这些 behavior For example, you just Make all this behavior. To like";
    let reply = r#"{"segments":[{"from":1,"to":4,"text":"You'd better "},{"from":5,"to":5,"text":"unify these "},{"from":6,"to":6,"text":"behaviors. "},{"from":7,"to":9,"text":"For example, "},{"from":10,"to":16,"text":"you just make all this behavior. "},{"from":17,"to":18,"text":"To like"}]}"#;
    let (result, host) = run_dictation(transcript, reply, Delivery::Inserted).await;
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(block_ids(&host.blocks()), ["text"]);
    assert_eq!(
        host.inserted(),
        ["You'd better unify these behaviors. For example, you just make all this behavior. To like"]
    );
    let Some(Instruction::PublishTranslation { pairs, .. }) = host.instructions().last().cloned() else {
        panic!("expected translation pairs")
    };
    assert_eq!(pairs, [pair("unify these", "统一这些"), pair("behaviors.", "behavior")]);
}

/// 输出顺序不受原文引用顺序限制：句末的「昨天」可移到句首，相邻冠词也可随翻译调整。
#[tokio::test]
async fn dictation_allows_sentence_word_order_changes_and_grammar() {
    let reply = r#"{"segments":[{"from":5,"to":5,"text":"Yesterday, "},{"from":1,"to":3,"text":"I went to "},{"from":4,"to":4,"text":"the zoo"},{"from":6,"to":6,"text":"."}]}"#;
    let (_, host) = run_dictation("I went to 动物园 昨天.", reply, Delivery::Inserted).await;
    assert_eq!(host.inserted(), ["Yesterday, I went to the zoo."]);
    assert_eq!(block_ids(&host.blocks()), ["text"]);
    let Some(Instruction::PublishTranslation { pairs, .. }) = host.instructions().last().cloned() else {
        panic!("expected translation pairs")
    };
    assert_eq!(pairs, [pair("Yesterday,", "昨天"), pair("the zoo", "动物园")]);
}

/// 不同识别服务产生的制表符、换行不应要求模型重抄；输出可正常排版，词对仍保留真实原文。
#[tokio::test]
async fn dictation_keeps_original_formatting_in_references_without_constraining_output_layout() {
    let reply = r#"{"segments":[{"from":1,"to":1,"text":"I "},{"from":2,"to":3,"text":"want an apple"},{"from":4,"to":4,"text":"."}]}"#;
    let (_, host) = run_dictation("I\t想要\r\n一个苹果.", reply, Delivery::Inserted).await;
    assert_eq!(host.inserted(), ["I want an apple."]);
    assert_eq!(block_ids(&host.blocks()), ["text"]);
    let Some(Instruction::PublishTranslation { pairs, .. }) = host.instructions().last().cloned() else {
        panic!("expected translation pairs")
    };
    assert_eq!(pairs, [pair("want an apple", "想要\r\n一个苹果")]);
}

/// 划词翻译输出译文与原文块，朗读目标语言一侧，并以「目标 · 本地」词对广播；回复缺少原文语言时失败。
#[tokio::test]
async fn selection_translates_speaks_and_publishes() {
    let (_directory, stores) = temporary_stores();
    let host =
        FakeHost::new("sayso.selection", r#"{"from":"en","translation":"意外发现"}"#, Delivery::Inserted, stores);
    let result = run(&built_in("sayso.selection"), &command("serendipity", InputSource::Selection), host.clone()).await;

    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(block_ids(&host.blocks()), ["translation", "original"]);
    assert_eq!(speaks(&host.instructions()), [("serendipity".to_string(), Language::English)]);
    assert_eq!(
        host.instructions().last(),
        Some(&Instruction::PublishTranslation { pairs: vec![pair("serendipity", "意外发现")], context: None })
    );

    // 第三种语言（不是目标语言也不是本地语言）：译为本地语言并显示，不朗读、不广播给生词本。
    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new("sayso.selection", r#"{"from":"fr","translation":"巅峰"}"#, Delivery::Inserted, stores);
    let result = run(&built_in("sayso.selection"), &command("apogée", InputSource::Selection), host.clone()).await;
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(block_ids(&host.blocks()), ["translation", "original"]);
    assert_eq!(host.instructions().len(), 2);

    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new("sayso.selection", r#"{"translation":"x"}"#, Delivery::Inserted, stores);
    let result = run(&built_in("sayso.selection"), &command("y", InputSource::Selection), host.clone()).await;
    assert!(matches!(result.status, RunStatus::Failed { .. }));
    assert!(host.blocks().is_empty());
}

/// 每个词条可单独加入；「全部加入」只加入候选框中尚未加入的词条，全部加入后显示完成提示；整句不是生词。
#[tokio::test]
async fn wordbook_adds_single_words_and_all_remaining() {
    let (_directory, stores) = temporary_stores();
    let wordbook = built_in("sayso.wordbook");

    async fn send(wordbook: &Plugin, stores: &Arc<Stores>, payload: Payload, surface: &[Block]) -> Vec<Block> {
        // 模拟候选框：扩展的块带上来源前缀。
        let blocks = surface.iter().cloned().map(|mut block| {
            block.id = format!("sayso.wordbook/{}", block.id);
            block
        });
        let event = Event::new(
            LanguagePair::default(),
            Some(SurfaceSnapshot { id: "s".into(), blocks: blocks.collect() }),
            payload,
        );
        let host = FakeHost::new("sayso.wordbook", "", Delivery::Inserted, stores.clone());
        let result = run(wordbook, &event, host.clone()).await;
        assert_eq!(result.status, RunStatus::Succeeded, "{result:?}");
        host.blocks()
    }
    fn invoke(block: &Block) -> Payload {
        let ActionKind::Invoke { payload } = &block.actions[0].kind else { panic!("expected invoke") };
        Payload::Action {
            action: ActionInvocation {
                block_id: block.id.clone(),
                payload: payload.clone(),
                origin: ActionOrigin::Candidate,
                record: None,
            },
        }
    }
    let translation = |pairs: Vec<TranslationPair>, context: Option<&str>| {
        Payload::Translation(Translation {
            input: Input { text: "I have a 苹果 and 香蕉.".into(), via: InputSource::Speech },
            pairs,
            context: context.map(Into::into),
            source: None,
        })
    };
    let words = || stores.records("sayso.wordbook", "words").unwrap();

    let offered =
        translation(vec![pair("an apple", "a 苹果"), pair("bananas", "香蕉")], Some("I have an apple and bananas."));
    let mut shown = send(&wordbook, &stores, offered, &[]).await;
    assert_eq!(block_ids(&shown), ["en|an apple", "en|bananas", "add-all"]);
    let labels: Vec<_> = shown.iter().map(|block| block.actions[0].label.as_deref().unwrap()).collect();
    assert_eq!(labels, ["加入生词本", "加入生词本", "全部加入生词本"]);
    assert!(words().is_empty());

    // 单独加入第一个词。
    let single = send(&wordbook, &stores, invoke(&shown[0]), &shown).await;
    assert_eq!(words().iter().map(|record| record.key.as_str()).collect::<Vec<_>>(), ["en|an apple"]);
    assert_eq!(words()[0].value("example"), Some("I have an apple and bananas.".into()));
    assert_eq!(block_ids(&single), ["en|an apple"]);
    assert!(single[0].actions.is_empty());
    shown[0] = single[0].clone();

    // 全部加入只补上剩下的词，并把按钮换成完成提示。
    let all = send(&wordbook, &stores, invoke(&shown[2]), &shown).await;
    let keys: HashSet<_> = words().into_iter().map(|record| record.key).collect();
    assert_eq!(keys, HashSet::from(["en|an apple".to_string(), "en|bananas".to_string()]));
    let apple = words().into_iter().find(|record| record.key == "en|an apple").unwrap();
    assert_eq!(apple.value("count"), Some(1.into()));
    assert_eq!(block_ids(&all), ["en|bananas", "add-all"]);
    let BlockContent::Status { message, .. } = &all[1].content else { panic!("expected status block") };
    assert_eq!(message, "已全部加入生词本");

    let paragraph =
        translation(vec![pair("This is a much longer sentence for you to read.", "这是一个长得多的句子。")], None);
    let long = send(&wordbook, &stores, paragraph, &[]).await;
    assert_eq!(long.len(), 1);
    assert_eq!(long[0].actions[0].label.as_deref(), Some("加入生词本"));
    send(&wordbook, &stores, invoke(&long[0]), &long).await;
    assert!(words()
        .iter()
        .any(|record| record.value("term") == Some("This is a much longer sentence for you to read.".into())));
}

/// 整句译文即使没有语言标签也必须产生真实词对，并经过生词本扩展生成可加入的块。
#[tokio::test]
async fn dictation_learning_blocks_do_not_depend_on_model_language_labels() {
    let transcript = "I have a 苹果.";
    let reply = r#"{"segments":[{"from":1,"to":5,"text":"I have an apple."}]}"#;
    let (_, dictation) = run_dictation(transcript, reply, Delivery::Inserted).await;
    assert_eq!(dictation.inserted(), ["I have an apple."]);
    let Some(Instruction::PublishTranslation { pairs, context }) = dictation.instructions().last().cloned() else {
        panic!("a translated sentence must publish learning pairs")
    };
    assert_eq!(pairs, [pair("an apple", "a 苹果")]);
    let (_directory, stores) = temporary_stores();
    let wordbook = FakeHost::new("sayso.wordbook", "", Delivery::Inserted, stores);
    let event = Event::new(
        LanguagePair::default(),
        None,
        Payload::Translation(Translation {
            input: Input { text: transcript.into(), via: InputSource::Speech },
            pairs,
            context,
            source: None,
        }),
    );
    assert_eq!(run(&built_in("sayso.wordbook"), &event, wordbook.clone()).await.status, RunStatus::Succeeded);
    let blocks = wordbook.blocks();
    assert_eq!(block_ids(&blocks), ["en|an apple"]);
    assert_eq!(blocks[0].actions[0].label.as_deref(), Some("加入生词本"));
}

/// 大小写、空白和标点变化不应产生额外的生词块。
#[tokio::test]
async fn dictation_learning_annotations_ignore_typographic_changes() {
    let reply = r#"{"segments":[{"from":1,"to":3,"text":"HELLO world! "},{"from":4,"to":5,"text":"an apple."}]}"#;
    let (_, host) = run_dictation("Hello world. 一个苹果。", reply, Delivery::Inserted).await;
    assert_eq!(host.inserted(), ["HELLO world! an apple."]);
    let Some(Instruction::PublishTranslation { pairs, .. }) = host.instructions().last().cloned() else {
        panic!("expected the actual translation pair")
    };
    assert_eq!(pairs, [pair("an apple", "一个苹果")]);
}

/// 真实服务曾在整句重排时省略独立标点编号，或将其输出置空；两者都不代表漏掉原文词语。
#[tokio::test]
async fn dictation_accepts_rewritten_punctuation_without_losing_learning_pairs() {
    for punctuation in ["", r#",{"from":16,"to":16,"text":""}"#] {
        let reply = format!(
            r#"{{"segments":[{{"from":1,"to":4,"text":"You'd better "}},{{"from":5,"to":5,"text":"unify these "}},{{"from":6,"to":6,"text":"behaviors. "}},{{"from":7,"to":9,"text":"For example, "}},{{"from":10,"to":12,"text":"you just make "}},{{"from":13,"to":15,"text":"all this behavior "}}{punctuation},{{"from":17,"to":18,"text":"to like"}}]}}"#
        );
        let (_, host) = run_dictation(
            "You'd better 统一这些 behavior For example, you just Make all this behavior. To like",
            &reply,
            Delivery::Inserted,
        )
        .await;
        assert_eq!(
            host.inserted(),
            ["You'd better unify these behaviors. For example, you just make all this behavior to like"]
        );
        assert_eq!(block_ids(&host.blocks()), ["text"]);
        assert!(
            matches!(host.instructions().last(), Some(Instruction::PublishTranslation { pairs, .. }) if !pairs.is_empty())
        );
    }
}

/// 中文可将句末 to me 的意思合入前面的「对我说」；引用后的空输出不能导致整句失败。
#[tokio::test]
async fn dictation_accepts_clause_reordering_without_per_span_word_output_requirements() {
    let (_directory, stores) = temporary_stores();
    let reply = r#"{"segments":[{"from":1,"to":2,"text":"她对我说"},{"from":3,"to":4,"text":"你好"},{"from":5,"to":7,"text":"。"}]}"#;
    let host = FakeHost::new("sayso.dictation", reply, Delivery::Inserted, stores);
    let mut event = command("She said nee how to me.", InputSource::Speech);
    event.languages = LanguagePair { target: Language::Chinese, native: Language::English };
    assert_eq!(run(&built_in("sayso.dictation"), &event, host.clone()).await.status, RunStatus::Succeeded);
    assert_eq!(host.inserted(), ["她对我说你好。"]);
    assert_eq!(block_ids(&host.blocks()), ["text"]);
    let Some(Instruction::PublishTranslation { pairs, .. }) = host.instructions().last().cloned() else {
        panic!("expected the restored target-language expression")
    };
    assert!(pairs.iter().any(|pair| pair.target == "你好" && pair.native == "nee how"));
    assert!(pairs.iter().all(|pair| pair.target.chars().any(char::is_alphanumeric)));
}

/// 在临时扩展目录中安装一个测试插件。
fn install(root: &std::path::Path, manifest: &str, files: &[(&str, &str)]) -> Plugin {
    let directory = root.join("plugin");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("extension.json"), manifest).unwrap();
    for (name, source) in files {
        std::fs::write(directory.join(name), source).unwrap();
    }
    let (mut plugins, problems) = load_installed(root, &HashSet::new());
    assert!(problems.is_empty(), "{problems:?}");
    plugins.remove(0)
}

fn manifest(capabilities: &str, timeout: u32) -> String {
    format!(
        r#"{{"schemaVersion":3,"id":"test.plugin","name":"Test","main":"main.js","timeoutSeconds":{timeout},
            "events":["command","translation"],"capabilities":{capabilities},"commands":[{{"id":"translate","title":"T"}}]}}"#
    )
}

/// 未声明的能力被拒绝并记为警告，插件仍能继续；相对导入只在扩展目录内解析；
/// 处理广播事件时不能写入其他应用。
#[tokio::test]
async fn runtime_enforces_capabilities_and_resolves_imports() {
    let root = tempfile::tempdir().unwrap();
    let plugin = install(
        root.path(),
        &manifest(r#"["blocks"]"#, 5),
        &[
            ("util.js", "export const label = (text) => `got ${text}`;"),
            (
                "main.js",
                r#"import { label } from "./util.js";
                export async function command(event) {
                  sayso.store.put("notes", "k", {});
                  let denied = "";
                  try { await sayso.llm("s", "u"); } catch (error) { denied = error.message; }
                  console.log("denied:", denied);
                  sayso.blocks.upsert({ id: "r", type: "status", message: label(event.input.text) });
                }
                export async function translation() { await sayso.insert("x"); }"#,
            ),
        ],
    );
    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new("test.plugin", "", Delivery::Inserted, stores);

    let result = run(&plugin, &command("hi", InputSource::Speech), host.clone()).await;
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert!(result.log.starts_with("denied: "), "{}", result.log);
    let BlockContent::Status { message, .. } = &host.blocks()[0].content else { panic!("expected status block") };
    assert_eq!(message, "got hi");
    assert_eq!(host.completions.load(Ordering::SeqCst), 0);

    let broadcast = Event::new(
        LanguagePair::default(),
        None,
        Payload::Translation(Translation {
            input: Input { text: "hi".into(), via: InputSource::Speech },
            pairs: vec![],
            context: None,
            source: None,
        }),
    );
    let result = run(&plugin, &broadcast, host.clone()).await;
    assert!(matches!(result.status, RunStatus::Failed { .. }), "{result:?}");
    assert!(host.inserted().is_empty());
}

/// 同步死循环也会在超时后被打断。
#[tokio::test]
async fn runtime_interrupts_busy_loops() {
    let root = tempfile::tempdir().unwrap();
    let plugin = install(root.path(), &manifest("[]", 1), &[("main.js", "export function command() { for (;;) {} }")]);
    let (_directory, stores) = temporary_stores();

    let result =
        run(&plugin, &command("hi", InputSource::Speech), FakeHost::new("test.plugin", "", Delivery::Inserted, stores))
            .await;
    assert_eq!(result.status, RunStatus::TimedOut);
}

/// 仓库中的示例扩展都能按 v3 加载，并能处理各自订阅的事件。
#[tokio::test]
async fn examples_load_and_handle_events() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/extensions");
    let (plugins, problems) = load_installed(&examples, &HashSet::new());
    assert!(problems.is_empty(), "{problems:?}");
    let example =
        |name: &str| plugins.iter().find(|plugin| plugin.id() == format!("com.sayso.example.{name}")).unwrap().clone();
    let (_directory, stores) = temporary_stores();

    // 待办：输入来自哪种路由都一样，记成一条待办。
    let host = FakeHost::new("com.sayso.example.todo", "", Delivery::Inserted, stores.clone());
    let mut event = command("Buy milk", InputSource::Selection);
    if let Payload::Command { command, .. } = &mut event.payload {
        command.id = "add".into();
    }
    assert_eq!(run(&example("todo"), &event, host.clone()).await.status, RunStatus::Succeeded);
    let todos = stores.records("com.sayso.example.todo", "todos").unwrap();
    assert_eq!(todos[0].fields["text"], "Buy milk");
    assert_eq!(todos[0].fields["app"], "Notes");

    // 收藏夹：每个短词对一张卡片，整句跳过；点「收藏」后累计次数。
    let host = FakeHost::new("com.sayso.example.favorites", "", Delivery::Inserted, stores.clone());
    let translated = Event::new(
        LanguagePair::default(),
        Some(SurfaceSnapshot { id: "s".into(), blocks: vec![] }),
        Payload::Translation(Translation {
            input: Input { text: "I have a 苹果.".into(), via: InputSource::Speech },
            pairs: vec![pair("an apple", "一个苹果"), pair("this is a much longer sentence", "整句")],
            context: Some("I have an apple.".into()),
            source: None,
        }),
    );
    assert_eq!(run(&example("favorites"), &translated, host.clone()).await.status, RunStatus::Succeeded);
    let cards = host.blocks();
    assert_eq!(block_ids(&cards), ["favorite-en|an apple"]);
    let ActionKind::Invoke { payload } = cards[0].actions[0].kind.clone() else { panic!("expected invoke") };
    let favorite = Event::new(
        LanguagePair::default(),
        Some(SurfaceSnapshot { id: "s".into(), blocks: vec![] }),
        Payload::Action {
            action: ActionInvocation {
                block_id: "favorite-en|an apple".into(),
                payload,
                origin: ActionOrigin::Candidate,
                record: None,
            },
        },
    );
    assert_eq!(run(&example("favorites"), &favorite, host.clone()).await.status, RunStatus::Succeeded);
    let favorites = stores.records("com.sayso.example.favorites", "favorites").unwrap();
    assert_eq!(favorites[0].fields["word"], "an apple");
    assert_eq!(favorites[0].fields["count"], 1.0);

    // Obsidian：写入的文字追加到库中当天的笔记。示例复制到临时目录，配置指向临时库。
    let copy = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    for file in ["extension.json", "main.js"] {
        std::fs::copy(examples.join("obsidian").join(file), copy.path().join(file)).unwrap();
    }
    let config = serde_json::json!({ "vault": vault.path(), "notePath": "Sayso/{date}.md" });
    std::fs::write(copy.path().join("config.json"), config.to_string()).unwrap();
    let obsidian = sayso_core::plugin::load_one(copy.path()).unwrap();
    let host = FakeHost::new("com.sayso.example.obsidian", "", Delivery::Inserted, stores.clone());
    let inserted = Event::new(
        LanguagePair::default(),
        None,
        Payload::Inserted {
            text: "I have an apple.".into(),
            destination: Some(AppInfo { id: None, name: Some("Notes".into()) }),
            delivery: Delivery::Inserted,
            by: "sayso.dictation".into(),
        },
    );
    assert_eq!(run(&obsidian, &inserted, host.clone()).await.status, RunStatus::Succeeded);
    let notes: Vec<_> = std::fs::read_dir(vault.path().join("Sayso")).unwrap().flatten().collect();
    let note = std::fs::read_to_string(notes[0].path()).unwrap();
    assert!(note.ends_with(" I have an apple. · Notes\n"), "{note}");
}
