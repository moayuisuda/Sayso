//! 内置插件与插件运行时的行为测试：用伪造的宿主固定模型回复，断言插件输出的指令、写入与投递。
mod common;

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sayso_core::error::CoreError;
use sayso_core::plugin::{built_in_plugins, load_installed, run, Host, Instruction, Plugin, RunResult, RunStatus};
use sayso_core::protocol::{
    ActionInvocation, ActionKind, ActionOrigin, AppInfo, Block, BlockAction, BlockContent, Delivery, Event,
    Input, InputSource, Language, LanguagePair, Payload, SurfaceSnapshot, TextStyle, Tone, Translation,
    TranslationPair,
};
use sayso_core::store::{StoreRecord, StoreWrite, Stores};

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

    /// 附加到其他块上的按钮：（块引用, 按钮）。
    fn attachments(&self) -> Vec<(String, Vec<BlockAction>)> {
        self.instructions()
            .into_iter()
            .filter_map(|instruction| match instruction {
                Instruction::AttachActions { block, actions } => Some((block, actions)),
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
    fn shell_lifetime(&self) -> Option<tokio::sync::watch::Receiver<()>> {
        None
    }

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

fn input_event(text: &str, via: InputSource) -> Event {
    Event::new(
        LanguagePair::default(),
        Some(SurfaceSnapshot { id: "s".into(), blocks: vec![] }),
        Payload::Input {
            input: Input { text: text.into(), via },
            source: Some(AppInfo { id: Some("com.apple.Notes".into()), name: Some("Notes".into()) }),
        },
    )
}

/// 后处理扩展广播的 translated hook，来自语音输入。
fn translated(input: &str, output: &str, pairs: Vec<TranslationPair>, context: Option<&str>) -> Event {
    Event::new(
        LanguagePair::default(),
        Some(SurfaceSnapshot { id: "s".into(), blocks: vec![] }),
        Payload::Translated(Translation {
            input: Input { text: input.into(), via: InputSource::Speech },
            output: output.into(),
            pairs,
            context: context.map(Into::into),
            card: Some("sayso.dictation/text".into()),
            source: Some(AppInfo { id: Some("com.apple.Notes".into()), name: Some("Notes".into()) }),
        }),
    )
}

/// 用户点击候选框中某个块的 invoke 按钮。
fn click(block: &Block) -> Event {
    let ActionKind::Invoke { payload } = block.actions[0].kind.clone() else { panic!("expected invoke") };
    Event::new(
        LanguagePair::default(),
        Some(SurfaceSnapshot { id: "s".into(), blocks: vec![] }),
        Payload::Action {
            action: ActionInvocation {
                block_id: block.id.clone(),
                payload,
                origin: ActionOrigin::Candidate,
                record: None,
            },
        },
    )
}

/// 最后一条指令是词对为空的 translated 广播。
fn no_pairs(host: &FakeHost) -> bool {
    matches!(host.instructions().last(), Some(Instruction::PublishTranslated { pairs, .. }) if pairs.is_empty())
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
    TranslationPair { target: target.into(), native: native.into(), card: None }
}

/// Sayso 广播时为词对填好的卡片引用。
fn carded(target: &str, native: &str, card: &str) -> TranslationPair {
    TranslationPair { card: Some(card.into()), ..pair(target, native) }
}

async fn run_dictation(transcript: &str, reply: &str, delivery: Delivery) -> (RunResult, Arc<FakeHost>) {
    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new("sayso.dictation", reply, delivery, stores);
    let result = run(&built_in("sayso.dictation"), &input_event(transcript, InputSource::Speech), host.clone()).await;
    (result, host)
}

/// 混说翻译后写入译文（译文可连带改写相邻词 a → an），输出文本块与剪贴板提示，朗读翻译块，最后广播词对。
/// 用给定的表运行应用背景扩展，返回它设置的背景。
async fn context_for(rows: &[(&str, Option<&str>, &str)], app: Option<&str>) -> Vec<Instruction> {
    let (_directory, stores) = temporary_stores();
    for (key, app_id, text) in rows {
        let record = serde_json::json!({ "appId": app_id, "text": text }).as_object().unwrap().clone();
        stores.apply("sayso.context", StoreWrite::Put { collection: "contexts".into(), key: (*key).into(), record }).unwrap();
    }
    let host = FakeHost::new("sayso.context", "", Delivery::Inserted, stores);
    let app = app.map(|id| AppInfo { id: Some(id.into()), name: None });
    let event = Event::new(LanguagePair::default(), None, Payload::Focus { app });
    assert_eq!(run(&built_in("sayso.context"), &event, host.clone()).await.status, RunStatus::Succeeded);
    host.instructions()
}

#[tokio::test]
async fn app_context_appends_the_app_row_to_the_default_row() {
    let rows = [("default", None, " base "), ("a", Some("id.xcode"), "swift"), ("b", Some("id.slack"), "  ")];
    let set = |text: &str| vec![Instruction::SetContext(text.into())];
    assert_eq!(context_for(&rows, Some("id.xcode")).await, set("base\nswift"));
    assert_eq!(context_for(&rows, Some("id.notes")).await, set("base"));
    assert_eq!(context_for(&rows, Some("id.slack")).await, set("base"));
    assert_eq!(context_for(&rows, None).await, set("base"));
    assert_eq!(context_for(&rows[1..], Some("id.xcode")).await, set("swift"));
    assert_eq!(context_for(&[], Some("id.xcode")).await, set(""));
}

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
        Some(&Instruction::PublishTranslated {
            output: "I have an apple.".into(),
            pairs: vec![pair("an apple", "a 苹果")],
            context: Some("I have an apple.".into()),
            card: "text".into(),
        })
    );
}

/// 外部替代命令沿用混说流程，一次模型请求得到整句与单词释义。
#[tokio::test]
async fn third_party_dictation_subscribes_to_speech_and_publishes_words() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let recipe = repo.join("examples/dictation-words");
    let dictation = repo.join("core/plugins/dictation");
    let directory = tempfile::tempdir().unwrap();
    for name in ["extension.json", "main.js"] {
        std::fs::copy(recipe.join(name), directory.path().join(name)).unwrap();
    }
    std::fs::copy(dictation.join("main.js"), directory.path().join("dictation.js")).unwrap();
    for name in ["prompt.js", "languages.js", "transcript.js"] {
        std::fs::copy(dictation.join(name), directory.path().join(name)).unwrap();
    }
    let plugin = sayso_core::plugin::load_one(directory.path()).unwrap();
    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new(
        plugin.id(),
        r#"{"segments":[{"from":1,"to":2,"text":"I have "},{"from":3,"to":4,"text":"an apple"},{"from":5,"to":5,"text":"."}],
            "words":[{"target":"have","native":"有"},{"target":"an","native":"一个"},{"target":"apple","native":"苹果"}]}"#,
        Delivery::Inserted,
        stores,
    );
    let result = run(&plugin, &input_event("I have a 苹果.", InputSource::Speech), host.clone()).await;
    assert_eq!(result.status, RunStatus::Succeeded, "{result:?}");
    assert_eq!(host.completions.load(Ordering::SeqCst), 1);
    assert_eq!(host.inserted(), ["I have an apple."]);
    assert_eq!(block_ids(&host.blocks()), ["text"]);
    assert_eq!(
        host.instructions().last(),
        Some(&Instruction::PublishTranslated {
            output: "I have an apple.".into(),
            pairs: vec![pair("an", "一个"), pair("apple", "苹果")],
            context: Some("I have an apple.".into()),
            card: "text".into(),
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
    let result = run(&built_in("sayso.dictation"), &input_event("I have a 苹果.", InputSource::Speech), host.clone()).await;
    assert_eq!(result.status, RunStatus::Succeeded);
    assert!(host.inserted().is_empty());
    let blocks = host.blocks();
    assert_eq!(block_ids(&blocks), ["text", "insertion"]);
    let BlockContent::Status { message, .. } = &blocks[1].content else { panic!("expected insertion error") };
    assert!(message.contains("Clipboard snapshot failed"));
    assert!(!message.contains("copied to clipboard"));
    assert!(host.instructions().iter().any(|instruction| matches!(instruction,
        Instruction::PublishTranslated { pairs, .. } if pairs == &[pair("an apple", "a 苹果")]
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

/// 纯目标语言输入不调用模型，原样写入；translated hook 照常广播，词对为空。
#[tokio::test]
async fn dictation_skips_model_for_pure_target_language() {
    let (result, host) = run_dictation("  I want an apple today.  ", "", Delivery::Inserted).await;

    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(host.completions.load(Ordering::SeqCst), 0);
    assert_eq!(host.inserted(), ["I want an apple today."]);
    assert_eq!(
        host.instructions().last(),
        Some(&Instruction::PublishTranslated {
            output: "I want an apple today.".into(),
            pairs: vec![],
            context: Some("I want an apple today.".into()),
            card: "text".into(),
        })
    );
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
    assert!(no_pairs(&host));
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
            let mut event = input_event("a 中 あ 한 я", InputSource::Speech);
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
            let mut event = input_event(transcript, InputSource::Speech);
            event.languages = LanguagePair { target, native };
            assert_eq!(run(&plugin, &event, host.clone()).await.status, RunStatus::Succeeded);
            assert_eq!(host.inserted(), [output], "{target:?}/{native:?}");
            let Some(Instruction::PublishTranslated { pairs, .. }) = host.instructions().last().cloned() else {
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
    let mut event = input_event("你好。わかりました。你好。我会去动物园。", InputSource::Speech);
    event.languages.target = Language::Japanese;
    let result = run(&built_in("sayso.dictation"), &event, host.clone()).await;
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(host.inserted(), ["こんにちは。わかりました。こんにちは。動物園に行きます。"]);
    let Some(Instruction::PublishTranslated { pairs, .. }) = host.instructions().last().cloned() else {
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
        assert!(no_pairs(&host));
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
    let Some(Instruction::PublishTranslated { pairs, .. }) = host.instructions().last().cloned() else {
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
    let Some(Instruction::PublishTranslated { pairs, .. }) = host.instructions().last().cloned() else {
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
    let Some(Instruction::PublishTranslated { pairs, .. }) = host.instructions().last().cloned() else {
        panic!("expected translation pairs")
    };
    assert_eq!(pairs, [pair("want an apple", "想要\r\n一个苹果")]);
}

/// 互译输出译文与原文块，朗读目标语言一侧，并以「目标 · 本地」词对广播；回复缺少原文语言时失败。
#[tokio::test]
async fn selection_translates_speaks_and_publishes() {
    let (_directory, stores) = temporary_stores();
    let host =
        FakeHost::new("sayso.selection", r#"{"from":"en","translation":"意外发现"}"#, Delivery::Inserted, stores);
    let result = run(&built_in("sayso.selection"), &input_event("serendipity", InputSource::Selection), host.clone()).await;

    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(block_ids(&host.blocks()), ["translation", "original"]);
    assert_eq!(speaks(&host.instructions()), [("serendipity".to_string(), Language::English)]);
    assert_eq!(
        host.instructions().last(),
        Some(&Instruction::PublishTranslated {
            output: "意外发现".into(),
            pairs: vec![pair("serendipity", "意外发现")],
            context: None,
            card: "translation".into(),
        })
    );

    // 第三种语言（不是目标语言也不是本地语言）：译为本地语言并显示，不朗读，广播的词对为空。
    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new("sayso.selection", r#"{"from":"fr","translation":"巅峰"}"#, Delivery::Inserted, stores);
    let result = run(&built_in("sayso.selection"), &input_event("apogée", InputSource::Selection), host.clone()).await;
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(block_ids(&host.blocks()), ["translation", "original"]);
    assert!(speaks(&host.instructions()).is_empty());
    assert_eq!(
        host.instructions().last(),
        Some(&Instruction::PublishTranslated {
            output: "巅峰".into(),
            pairs: vec![],
            context: None,
            card: "translation".into()
        })
    );

    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new("sayso.selection", r#"{"translation":"x"}"#, Delivery::Inserted, stores);
    let result = run(&built_in("sayso.selection"), &input_event("y", InputSource::Selection), host.clone()).await;
    assert!(matches!(result.status, RunStatus::Failed { .. }));
    assert!(host.blocks().is_empty());
}

/// 在每张词条卡片上附加「加入生词本」，多个词条另给「全部加入」；同一候选框中每个词条只计一次，整句也可加入。
#[tokio::test]
async fn wordbook_attaches_add_buttons_and_counts_each_word_once_per_candidate_box() {
    let (_directory, stores) = temporary_stores();
    let wordbook = built_in("sayso.wordbook");

    /// 返回本次附加的按钮，生词本不输出自己的块。
    async fn send(
        wordbook: &Plugin,
        stores: &Arc<Stores>,
        surface: &str,
        payload: Payload,
    ) -> Vec<(String, Vec<BlockAction>)> {
        let event =
            Event::new(LanguagePair::default(), Some(SurfaceSnapshot { id: surface.into(), blocks: vec![] }), payload);
        let host = FakeHost::new("sayso.wordbook", "", Delivery::Inserted, stores.clone());
        let result = run(wordbook, &event, host.clone()).await;
        assert_eq!(result.status, RunStatus::Succeeded, "{result:?}");
        assert!(host.blocks().is_empty());
        host.attachments()
    }
    /// 点击附加按钮时，blockId 是卡片引用。
    fn invoke((card, actions): &(String, Vec<BlockAction>)) -> Payload {
        let ActionKind::Invoke { payload } = &actions[0].kind else { panic!("expected invoke") };
        Payload::Action {
            action: ActionInvocation {
                block_id: card.clone(),
                payload: payload.clone(),
                origin: ActionOrigin::Candidate,
                record: None,
            },
        }
    }
    fn labels(attachments: &[(String, Vec<BlockAction>)]) -> Vec<(&str, &str, bool)> {
        attachments
            .iter()
            .map(|(card, actions)| (card.as_str(), actions[0].label.as_deref().unwrap(), actions[0].disabled))
            .collect()
    }
    let translation = |pairs: Vec<TranslationPair>, context: Option<&str>| {
        Payload::Translated(Translation {
            input: Input { text: "I have a 苹果 and 香蕉.".into(), via: InputSource::Speech },
            output: context.unwrap_or_default().into(),
            pairs,
            context: context.map(Into::into),
            card: Some("sayso.dictation/text".into()),
            source: None,
        })
    };
    let words = || stores.records("sayso.wordbook", "words").unwrap();
    let count = |key: &str| words().into_iter().find(|record| record.key == key).unwrap().value("count");
    let (text, apple, bananas) = ("sayso.dictation/text", "sayso.dictation/@term-0", "sayso.dictation/@term-1");

    // 同一词条的词对共用卡片，只附加一次；多个词条时整句卡片上有「全部加入」。
    let offered = translation(
        vec![
            carded("an apple", "a 苹果", apple),
            carded("bananas", "香蕉", bananas),
            carded("An apple", "a 苹果", apple),
        ],
        Some("I have an apple and bananas."),
    );
    let attached = send(&wordbook, &stores, "s", offered).await;
    assert_eq!(
        labels(&attached),
        [(apple, "加入生词本", false), (bananas, "加入生词本", false), (text, "全部加入生词本", false)]
    );
    assert!(words().is_empty());

    // 单独加入第一个词，按钮换成不可点击的次数；还有词未加入，「全部加入」不变。
    let after = send(&wordbook, &stores, "s", invoke(&attached[0])).await;
    assert_eq!(words().iter().map(|record| record.key.as_str()).collect::<Vec<_>>(), ["en|an apple"]);
    assert_eq!(words()[0].value("example"), Some("I have an apple and bananas.".into()));
    assert_eq!(labels(&after), [(apple, "已加入生词本（第 1 次）", true)]);

    // 全部加入只给剩下的词计数，并把「全部加入」换成完成状态。
    let after = send(&wordbook, &stores, "s", invoke(&attached[2])).await;
    assert_eq!(count("en|an apple"), Some(1.into()));
    assert_eq!(count("en|bananas"), Some(1.into()));
    assert_eq!(
        labels(&after),
        [
            (apple, "已加入生词本（第 1 次）", true),
            (bananas, "已加入生词本（第 1 次）", true),
            (text, "已全部加入生词本", true)
        ]
    );

    // 另一个候选框再次遇到同一个词时累计次数；逐个加入完所有词时「全部加入」同样换成完成状态；整句也提供加入入口。
    let paragraph = translation(
        vec![
            carded("an apple", "一个苹果", apple),
            carded("This is a much longer sentence.", "这是一个长得多的句子。", bananas),
        ],
        None,
    );
    let attached = send(&wordbook, &stores, "t", paragraph).await;
    send(&wordbook, &stores, "t", invoke(&attached[0])).await;
    assert_eq!(count("en|an apple"), Some(2.into()));
    let after = send(&wordbook, &stores, "t", invoke(&attached[1])).await;
    assert_eq!(after.last().map(|(card, _)| card.as_str()), Some(text));
    assert!(words().iter().any(|record| record.value("term") == Some("This is a much longer sentence.".into())));
}

/// 整句译文即使没有语言标签也必须产生真实词对，生词本据此在词条卡片上附加按钮。
#[tokio::test]
async fn dictation_learning_pairs_do_not_depend_on_model_language_labels() {
    let transcript = "I have a 苹果.";
    let reply = r#"{"segments":[{"from":1,"to":5,"text":"I have an apple."}]}"#;
    let (_, dictation) = run_dictation(transcript, reply, Delivery::Inserted).await;
    assert_eq!(dictation.inserted(), ["I have an apple."]);
    let Some(Instruction::PublishTranslated { output, pairs, context, .. }) = dictation.instructions().last().cloned()
    else {
        panic!("a translated sentence must publish learning pairs")
    };
    assert_eq!(pairs, [pair("an apple", "a 苹果")]);
    // 广播时 Sayso 为词对填上卡片引用。
    let pairs =
        pairs.into_iter().map(|pair| TranslationPair { card: Some("sayso.dictation/@term-0".into()), ..pair }).collect();
    let (_directory, stores) = temporary_stores();
    let wordbook = FakeHost::new("sayso.wordbook", "", Delivery::Inserted, stores);
    let event = Event::new(
        LanguagePair::default(),
        Some(SurfaceSnapshot { id: "s".into(), blocks: vec![] }),
        Payload::Translated(Translation {
            input: Input { text: transcript.into(), via: InputSource::Speech },
            output,
            pairs,
            context,
            card: Some("sayso.dictation/text".into()),
            source: None,
        }),
    );
    assert_eq!(run(&built_in("sayso.wordbook"), &event, wordbook.clone()).await.status, RunStatus::Succeeded);
    let attached = wordbook.attachments();
    assert_eq!(attached.len(), 1);
    assert_eq!(attached[0].0, "sayso.dictation/@term-0");
    assert_eq!(attached[0].1[0].label.as_deref(), Some("加入生词本"));
}

/// 大小写、空白和标点变化不应产生额外的生词块。
#[tokio::test]
async fn dictation_learning_annotations_ignore_typographic_changes() {
    let reply = r#"{"segments":[{"from":1,"to":3,"text":"HELLO world! "},{"from":4,"to":5,"text":"an apple."}]}"#;
    let (_, host) = run_dictation("Hello world. 一个苹果。", reply, Delivery::Inserted).await;
    assert_eq!(host.inserted(), ["HELLO world! an apple."]);
    let Some(Instruction::PublishTranslated { pairs, .. }) = host.instructions().last().cloned() else {
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
            matches!(host.instructions().last(), Some(Instruction::PublishTranslated { pairs, .. }) if !pairs.is_empty())
        );
    }
}

/// 中文可将句末 to me 的意思合入前面的「对我说」；引用后的空输出不能导致整句失败。
#[tokio::test]
async fn dictation_accepts_clause_reordering_without_per_span_word_output_requirements() {
    let (_directory, stores) = temporary_stores();
    let reply = r#"{"segments":[{"from":1,"to":2,"text":"她对我说"},{"from":3,"to":4,"text":"你好"},{"from":5,"to":7,"text":"。"}]}"#;
    let host = FakeHost::new("sayso.dictation", reply, Delivery::Inserted, stores);
    let mut event = input_event("She said nee how to me.", InputSource::Speech);
    event.languages = LanguagePair { target: Language::Chinese, native: Language::English };
    assert_eq!(run(&built_in("sayso.dictation"), &event, host.clone()).await.status, RunStatus::Succeeded);
    assert_eq!(host.inserted(), ["她对我说你好。"]);
    assert_eq!(block_ids(&host.blocks()), ["text"]);
    let Some(Instruction::PublishTranslated { pairs, .. }) = host.instructions().last().cloned() else {
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
    let plugin = plugins.remove(0);
    if plugin.manifest.grants(sayso_core::protocol::Capability::Shell) {
        common::install_shell_executable();
    }
    plugin
}

fn manifest(capabilities: &str, timeout: u32) -> String {
    format!(
        r#"{{"schemaVersion":4,"id":"test.plugin","name":"Test","main":"main.js","timeoutSeconds":{timeout},
            "events":["translated","action"],"capabilities":{capabilities}}}"#
    )
}

/// 扩展使用 input 订阅文本；已移除的 command 不能再注册。
#[test]
fn extensions_subscribe_to_input_events() {
    for (extra, loads) in [
        (r#""events":["input"],"capabilities":["publish"]"#, true),
        (r#""events":["command"]"#, false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("plugin");
        std::fs::create_dir_all(&directory).unwrap();
        let manifest = format!(r#"{{"schemaVersion":4,"id":"test.plugin","name":"Test","main":"main.js",{extra}}}"#);
        std::fs::write(directory.join("extension.json"), manifest).unwrap();
        std::fs::write(directory.join("main.js"), "").unwrap();
        let (plugins, problems) = load_installed(root.path(), &HashSet::new());
        assert_eq!((plugins.len(), problems.len()), if loads { (1, 0) } else { (0, 1) }, "{extra}");
    }
}

/// 未声明的能力被拒绝并记为警告，插件仍能继续；相对导入只在扩展目录内解析；
/// 只有 input 处理函数能直接写入其他应用。
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
                export async function translated(event) {
                  sayso.store.put("notes", "k", {});
                  let denied = "";
                  try { await sayso.llm("s", "u"); } catch (error) { denied = error.message; }
                  console.log("denied:", denied);
                  sayso.blocks.upsert({ id: "r", type: "status", message: label(event.output),
                    actions: [{ type: "invoke", label: "Go" }] });
                }
                export async function action() { await sayso.insert("x"); }"#,
            ),
        ],
    );
    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new("test.plugin", "", Delivery::Inserted, stores);

    let result = run(&plugin, &translated("hi", "hi", vec![], None), host.clone()).await;
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert!(result.log.starts_with("denied: "), "{}", result.log);
    let BlockContent::Status { message, .. } = &host.blocks()[0].content else { panic!("expected status block") };
    assert_eq!(message, "got hi");
    assert_eq!(host.completions.load(Ordering::SeqCst), 0);

    let result = run(&plugin, &click(&host.blocks()[0]), host.clone()).await;
    assert!(matches!(result.status, RunStatus::Failed { .. }), "{result:?}");
    assert!(host.inserted().is_empty());
}

/// 同步死循环也会在超时后被打断。
#[tokio::test]
async fn runtime_interrupts_busy_loops() {
    let root = tempfile::tempdir().unwrap();
    let plugin =
        install(root.path(), &manifest("[]", 1), &[("main.js", "export function translated() { for (;;) {} }")]);
    let (_directory, stores) = temporary_stores();

    let result = run(
        &plugin,
        &translated("hi", "hi", vec![], None),
        FakeHost::new("test.plugin", "", Delivery::Inserted, stores),
    )
    .await;
    assert_eq!(result.status, RunStatus::TimedOut);
}

/// 权限必须在解析或执行脚本之前校验，不能通过文件副作用绕过。
#[tokio::test]
async fn shell_requires_its_own_capability() {
    let root = tempfile::tempdir().unwrap();
    let plugin = install(
        root.path(),
        &manifest(r#"["fs"]"#, 5),
        &[(
            "main.js",
            r#"
        export async function translated() {
          try { await sayso.shell.run("echo denied > denied.txt"); }
          catch (error) { console.log(error.message); }
        }
    "#,
        )],
    );
    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new("test.plugin", "", Delivery::Inserted, stores);
    let result = run(&plugin, &translated("", "", vec![], None), host).await;
    assert_eq!(result.status, RunStatus::Succeeded);
    assert!(result.log.contains("Capability not declared: shell"), "{result:?}");
    assert!(!plugin.directory().unwrap().join("denied.txt").exists());
}

/// 真正执行统一 shell：覆盖管道、重定向、Unicode stdin、环境变量及含空格的相对工作目录。
#[tokio::test]
async fn shell_runs_portable_scripts_and_preserves_results() {
    let root = tempfile::tempdir().unwrap();
    let plugin = install(
        root.path(),
        &manifest(r#"["shell"]"#, 5),
        &[(
            "main.js",
            r#"
        export async function translated() {
          const result = await sayso.shell.run(
            'cat | head -n 1 > input.txt; echo "$SAYSO_TEST_VALUE"; cat input.txt; echo issue >&2; exit 7',
            { cwd: "工作 空间", env: { SAYSO_TEST_VALUE: "环境 🍎" }, stdin: "中文 ; $literal\n第二行\n" }
          );
          if (result.exitCode !== 7 || result.stdout !== "环境 🍎\n中文 ; $literal\n" || result.stderr !== "issue\n")
            throw new Error(JSON.stringify(result));
          const missing = await sayso.shell.run("sayso_missing_test_command");
          if (missing.exitCode !== 127 || !missing.stderr) throw new Error("Missing command was not reported");
          const next = await sayso.shell.run("echo $SAYSO_TEST_VALUE");
          if (next.stdout !== "\n") throw new Error("Environment leaked between calls");
        }
    "#,
        )],
    );
    let workspace = plugin.directory().unwrap().join("工作 空间");
    std::fs::create_dir(&workspace).unwrap();
    let (_directory, stores) = temporary_stores();
    let result =
        run(&plugin, &translated("", "", vec![], None), FakeHost::new("test.plugin", "", Delivery::Inserted, stores))
            .await;
    assert_eq!(result.status, RunStatus::Succeeded, "{result:?}");
    assert_eq!(std::fs::read_to_string(workspace.join("input.txt")).unwrap(), "中文 ; $literal\n");
}

/// stdout 与 stderr 同时排空；超限时中断，不保留无限增长的字符串。
#[tokio::test]
async fn shell_bounds_output_and_rejects_invalid_requests() {
    let root = tempfile::tempdir().unwrap();
    let plugin = install(
        root.path(),
        &manifest(r#"["shell"]"#, 5),
        &[(
            "main.js",
            r#"
        export async function translated() {
          for (const [script, options, expected] of [
            ["echo (", {}, "Shell:"],
            ["echo ok", { timeoutMs: 0 }, "timeoutMs"],
            ["echo ok", { env: { "BAD=KEY": "x" } }, "environment"],
            ["echo ok", { stdin: "x".repeat(1048577) }, "stdin"],
            ["cat large.txt & cat large.txt >&2", {}, "1 MiB"],
          ]) {
            let rejected = false;
            try { await sayso.shell.run(script, options); }
            catch (error) { rejected = error.message.includes(expected); }
            if (!rejected) throw new Error(`Expected rejection: ${expected}`);
          }
          const recovered = await sayso.shell.run("echo recovered");
          if (recovered.stdout !== "recovered\n") throw new Error("Output limit broke later calls");
          const binary = await sayso.shell.run("cat binary.bin; echo issue >&2");
          if (binary.stdout.length !== 600000 || binary.stderr !== "issue\n")
            throw new Error("UTF-8 replacement must not change the byte budget");
        }
    "#,
        )],
    );
    std::fs::write(plugin.directory().unwrap().join("large.txt"), vec![b'x'; 800_000]).unwrap();
    std::fs::write(plugin.directory().unwrap().join("binary.bin"), vec![0xff; 600_000]).unwrap();
    let (_directory, stores) = temporary_stores();
    let result =
        run(&plugin, &translated("", "", vec![], None), FakeHost::new("test.plugin", "", Delivery::Inserted, stores))
            .await;
    assert_eq!(result.status, RunStatus::Succeeded, "{result:?}");
}

/// 独立测试进程持续写入心跳，供取消测试验证实际进程已退出，而非只结束等待。
#[test]
#[ignore = "invoked only as the real subprocess fixture"]
fn shell_child_process() {
    use std::io::Write;
    let marker = std::env::var_os("SAYSO_SHELL_TEST_MARKER").unwrap();
    let _child = std::env::var_os("SAYSO_SHELL_TEST_CHILD_MARKER").map(|marker| {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "shell_child_process", "--nocapture"])
            .env("SAYSO_SHELL_TEST_MARKER", marker)
            .env_remove("SAYSO_SHELL_TEST_CHILD_MARKER")
            .spawn()
            .unwrap()
    });
    let noisy = std::env::var("SAYSO_SHELL_TEST_OUTPUT").is_ok_and(|value| value == "1");
    let mut file = std::fs::OpenOptions::new().append(true).create(true).open(marker).unwrap();
    loop {
        file.write_all(b"tick\n").unwrap();
        file.flush().unwrap();
        if noisy {
            std::io::stdout().write_all(&[b'x'; 131072]).unwrap();
            std::io::stderr().write_all(&[b'y'; 131072]).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn shell_process_script() -> String {
    let executable = std::env::current_exe().unwrap().to_string_lossy().replace('\\', "/").replace('\'', "'\"'\"'");
    format!("'{executable}' --ignored --exact shell_child_process --nocapture")
}

async fn wait_for_shell_marker(marker: &std::path::Path) {
    tokio::time::timeout(std::time::Duration::from_secs(4), async {
        while !marker.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("shell subprocess must start");
}

async fn assert_shell_process_stopped(marker: &std::path::Path) {
    let length = std::fs::metadata(marker).unwrap().len();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(std::fs::metadata(marker).unwrap().len(), length, "shell subprocess kept running after cancellation");
}

/// 每次调用的超时、整个事件的超时与未 await 的脚本都必须结束真实外部进程。
#[tokio::test]
async fn shell_stops_processes_on_timeouts_and_event_completion() {
    for mode in ["call", "event", "unawaited", "tree", "output"] {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("heartbeat");
        let script = serde_json::to_string(&shell_process_script()).unwrap();
        let child_marker = root.path().join("child-heartbeat");
        let mut env = serde_json::json!({ "SAYSO_SHELL_TEST_MARKER": marker });
        if mode == "tree" {
            env["SAYSO_SHELL_TEST_CHILD_MARKER"] = serde_json::json!(child_marker);
        }
        if mode == "output" {
            env["SAYSO_SHELL_TEST_OUTPUT"] = serde_json::json!("1");
        }
        let env = env.to_string();
        let body = match mode {
            "call" | "tree" => format!("await sayso.shell.run({script}, {{ env: {env}, timeoutMs: 3000 }});"),
            "event" | "output" => format!("await sayso.shell.run({script}, {{ env: {env} }});"),
            _ => format!(
                r#"
                sayso.shell.run({script}, {{ env: {env} }}).catch(() => {{}});
                while (!sayso.fs.exists({marker})) await sayso.shell.run("sleep 0.02");
            "#,
                marker = serde_json::to_string(&marker).unwrap()
            ),
        };
        let source = format!("export async function translated() {{ {body} }}");
        let plugin = install(
            root.path(),
            &manifest(r#"["shell","fs"]"#, if mode == "event" { 4 } else { 10 }),
            &[("main.js", &source)],
        );
        let (_directory, stores) = temporary_stores();
        let result = run(
            &plugin,
            &translated("", "", vec![], None),
            FakeHost::new("test.plugin", "", Delivery::Inserted, stores),
        )
        .await;
        match mode {
            "call" | "tree" => assert!(
                matches!(result.status, RunStatus::Failed { ref message } if message.contains("timed out")),
                "{result:?}"
            ),
            "event" => assert_eq!(result.status, RunStatus::TimedOut, "{result:?}"),
            "output" => assert!(
                matches!(result.status, RunStatus::Failed { ref message } if message.contains("1 MiB")),
                "{result:?}"
            ),
            _ => assert_eq!(result.status, RunStatus::Succeeded, "{result:?}"),
        }
        assert!(marker.exists(), "the subprocess must have actually run: {mode} {result:?}");
        assert_shell_process_stopped(&marker).await;
        if mode == "tree" {
            assert!(child_marker.exists(), "the real subprocess must have spawned its own child");
            assert_shell_process_stopped(&child_marker).await;
        }
    }
}

/// 宿主丢弃事件 Future 时，RAII 取消信号仍然能结束工作线程内的进程。
#[tokio::test]
async fn shell_stops_processes_when_the_invocation_is_aborted() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("heartbeat");
    let source = format!(
        "export async function translated() {{ await sayso.shell.run({}, {{ env: {} }}); }}",
        serde_json::to_string(&shell_process_script()).unwrap(),
        serde_json::json!({ "SAYSO_SHELL_TEST_MARKER": marker })
    );
    let plugin = install(root.path(), &manifest(r#"["shell"]"#, 5), &[("main.js", &source)]);
    let (_directory, stores) = temporary_stores();
    let host = FakeHost::new("test.plugin", "", Delivery::Inserted, stores);
    let task = tokio::spawn(async move { run(&plugin, &translated("", "", vec![], None), host).await });
    wait_for_shell_marker(&marker).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    // 工作线程通过异步信号回收进程，先等待一次调度再检查心跳不再增长。
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_shell_process_stopped(&marker).await;
}

/// 仓库中的示例扩展都能按 v4 加载，并能处理各自订阅的 hook 与按钮回调。
#[tokio::test]
async fn examples_load_and_handle_events() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/extensions");
    let (plugins, problems) = load_installed(&examples, &HashSet::new());
    assert!(problems.is_empty(), "{problems:?}");
    let example =
        |name: &str| plugins.iter().find(|plugin| plugin.id() == format!("com.sayso.example.{name}")).unwrap().clone();
    let (_directory, stores) = temporary_stores();
    let sentence = || {
        translated(
            "I have a 苹果.",
            "I have an apple.",
            vec![
                carded("an apple", "一个苹果", "sayso.dictation/@term-0"),
                carded("this is a much longer sentence", "整句", "sayso.dictation/@term-1"),
            ],
            Some("I have an apple."),
        )
    };
    // 点击附加在卡片上的按钮。
    let click_attached = |(card, actions): &(String, Vec<BlockAction>)| {
        click(&Block { id: card.clone(), actions: actions.clone(), ..Block::status("", "", Tone::Info, "") })
    };

    // 收藏夹：只在短词的卡片上附加「收藏」，整句跳过；点击后累计次数，按钮换成不可点击的「已收藏」。
    let host = FakeHost::new("com.sayso.example.favorites", "", Delivery::Inserted, stores.clone());
    assert_eq!(run(&example("favorites"), &sentence(), host.clone()).await.status, RunStatus::Succeeded);
    let attached = host.attachments();
    assert_eq!(attached.iter().map(|(card, _)| card.as_str()).collect::<Vec<_>>(), ["sayso.dictation/@term-0"]);
    assert_eq!(
        run(&example("favorites"), &click_attached(&attached[0]), host.clone()).await.status,
        RunStatus::Succeeded
    );
    let favorites = stores.records("com.sayso.example.favorites", "favorites").unwrap();
    assert_eq!(favorites[0].fields["word"], "an apple");
    assert_eq!(favorites[0].fields["count"], 1.0);
    let (card, saved) = host.attachments().pop().unwrap();
    assert_eq!((card.as_str(), saved[0].disabled), ("sayso.dictation/@term-0", true));

    // 语法讲解：翻译完毕只在整句卡片上附加按钮，点击后才调用模型讲解写入的整句。
    let host = FakeHost::new("com.sayso.example.explain", "• Present simple.", Delivery::Inserted, stores.clone());
    assert_eq!(run(&example("explain"), &sentence(), host.clone()).await.status, RunStatus::Succeeded);
    assert_eq!(host.completions.load(Ordering::SeqCst), 0);
    let attached = host.attachments();
    assert_eq!(attached.iter().map(|(card, _)| card.as_str()).collect::<Vec<_>>(), ["sayso.dictation/text"]);
    assert_eq!(
        run(&example("explain"), &click_attached(&attached[0]), host.clone()).await.status,
        RunStatus::Succeeded
    );
    let explained = host.blocks().pop().unwrap();
    let BlockContent::Card { title, body, .. } = &explained.content else { panic!("expected card") };
    assert_eq!((title.as_str(), body.as_deref()), ("I have an apple.", Some("• Present simple.")));

    // Obsidian：在每张词条卡片上附加按钮，点击后把词对与整句写成 callout 卡片，追加到库中当天的笔记。示例复制到临时目录，配置指向临时库。
    let copy = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    for file in ["extension.json", "main.js"] {
        std::fs::copy(examples.join("obsidian").join(file), copy.path().join(file)).unwrap();
    }
    let config = serde_json::json!({ "vault": vault.path(), "notePath": "Sayso/{date}.md" });
    std::fs::write(copy.path().join("config.json"), config.to_string()).unwrap();
    let obsidian = sayso_core::plugin::load_one(copy.path()).unwrap();
    let host = FakeHost::new("com.sayso.example.obsidian", "", Delivery::Inserted, stores.clone());
    assert_eq!(run(&obsidian, &sentence(), host.clone()).await.status, RunStatus::Succeeded);
    assert!(!vault.path().join("Sayso").exists());
    // 整句卡片与两张词条卡片各有一个按钮；点整句卡片上的记下整次翻译。
    let attached = host.attachments();
    let cards: Vec<_> = attached.iter().map(|(card, _)| card.as_str()).collect();
    assert_eq!(cards, ["sayso.dictation/text", "sayso.dictation/@term-0", "sayso.dictation/@term-1"]);
    assert_eq!(run(&obsidian, &click_attached(&attached[0]), host.clone()).await.status, RunStatus::Succeeded);
    let notes: Vec<_> = std::fs::read_dir(vault.path().join("Sayso")).unwrap().flatten().collect();
    let note = std::fs::read_to_string(notes[0].path()).unwrap();
    let card = note.trim_start().split_once('\n').map(|(title, body)| (title.to_string(), body.to_string()));
    let (title, body) = card.unwrap();
    assert!(title.starts_with("> [!note] ") && title.ends_with(" · Notes"), "{note}");
    assert_eq!(
        body,
        "> I have ==an apple==.\n>\n> - ==an apple== · 一个苹果\n> - ==this is a much longer sentence== · 整句\n"
    );
    let (card, saved) = host.attachments().pop().unwrap();
    assert_eq!((card.as_str(), saved[0].disabled), ("sayso.dictation/text", true));

    // 词条卡片上的按钮只记这个词条，附上高亮该词条的整句。
    assert_eq!(run(&obsidian, &click_attached(&attached[1]), host.clone()).await.status, RunStatus::Succeeded);
    let note = std::fs::read_to_string(notes[0].path()).unwrap();
    assert!(note.ends_with("> ==an apple== · 一个苹果\n>\n> I have ==an apple==.\n"), "{note}");
}
