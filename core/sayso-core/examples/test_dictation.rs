//! 回放文本调用真实翻译服务及正式内置插件；密钥从 stdin 读取，不写入文件或命令参数。
use async_trait::async_trait;
use sayso_core::error::CoreError;
use sayso_core::llm::Endpoint;
use sayso_core::plugin::{built_in_plugins, run, Host, Instruction, RunStatus};
use sayso_core::protocol::{
    BlockContent, CommandRef, Delivery, Event, Input, InputSource, LanguagePair, Payload, Translation,
};
use sayso_core::settings::{llm_endpoint, SecretSlot, Secrets, Settings};
use sayso_core::store::StoreRecord;
use std::io::BufRead;
use std::sync::{Arc, Mutex};

struct ReplayHost {
    endpoint: Endpoint,
    http: reqwest::Client,
    output: Mutex<String>,
    instructions: Mutex<Vec<Instruction>>,
    replies: Mutex<Vec<String>>,
}

#[async_trait]
impl Host for ReplayHost {
    fn apply(&self, instruction: Instruction) -> Result<(), CoreError> {
        // 记录正式插件的指令；回放不访问 UI、播放设备或用户的生词本。
        self.instructions.lock().unwrap().push(instruction);
        Ok(())
    }
    async fn complete(&self, system: String, user: String) -> Result<String, CoreError> {
        let reply = self.endpoint.complete(&self.http, &system, &user).await?;
        self.replies.lock().unwrap().push(reply.clone());
        Ok(reply)
    }
    async fn insert(&self, text: String) -> Result<Delivery, CoreError> {
        // 回放只记录正式插件生成的输入文本，不写入用户当前应用。
        *self.output.lock().unwrap() = text;
        Ok(Delivery::Inserted)
    }
    fn records(&self, _collection: &str) -> Result<Vec<StoreRecord>, CoreError> {
        Err(CoreError::InvalidInstruction("dictation replay does not provide stores".into()))
    }
}

#[derive(serde::Deserialize)]
struct Case {
    transcript: String,
    languages: LanguagePair,
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    excludes: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("Usage: test_dictation settings.json cases.json; API key on stdin".into());
    }
    let settings: Settings = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let mut key = String::new();
    std::io::stdin().lock().read_line(&mut key)?;
    let mut secrets = Secrets::default();
    secrets.set(SecretSlot::Llm { provider: settings.llm_provider }, key.trim().into());
    let endpoint = llm_endpoint(&settings, &secrets).ok_or("Incomplete LLM configuration")?;
    let plugins = built_in_plugins();
    let plugin = plugins.iter().find(|plugin| plugin.id() == "sayso.dictation").unwrap();
    let wordbook = plugins.iter().find(|plugin| plugin.id() == "sayso.wordbook").unwrap();
    let mut failures = 0;
    for case in cases {
        let mut failed = false;
        if !case.languages.is_valid() {
            return Err("Target and native languages must differ".into());
        }
        let host = Arc::new(ReplayHost {
            endpoint: endpoint.clone(),
            http: reqwest::Client::new(),
            output: Mutex::default(),
            instructions: Mutex::default(),
            replies: Mutex::default(),
        });
        let event = Event::new(
            case.languages,
            None,
            Payload::Command {
                command: CommandRef { id: "translate".into() },
                input: Input { text: case.transcript.clone(), via: InputSource::Speech },
                source: None,
            },
        );
        let started = std::time::Instant::now();
        let result = run(plugin, &event, host.clone()).await;
        let published = host.instructions.lock().unwrap().iter().find_map(|instruction| match instruction {
            Instruction::PublishTranslation { pairs, context } => Some((pairs.clone(), context.clone())),
            _ => None,
        });
        if let Some((pairs, context)) = published {
            let translation = Event::new(
                case.languages,
                None,
                Payload::Translation(Translation {
                    input: Input { text: case.transcript.clone(), via: InputSource::Speech },
                    pairs,
                    context,
                    source: None,
                }),
            );
            if run(wordbook, &translation, host.clone()).await.status != RunStatus::Succeeded {
                failed = true;
            }
        }
        let instructions = host.instructions.lock().unwrap();
        let blocks: Vec<_> = instructions
            .iter()
            .filter_map(|instruction| match instruction {
                Instruction::UpsertBlock(block) => Some(block),
                _ => None,
            })
            .collect();
        let notice = blocks.iter().find(|block| block.id == "notice");
        if result.status != RunStatus::Succeeded || notice.is_some() {
            failed = true;
        }
        let highlights: Vec<_> = blocks
            .iter()
            .filter_map(|block| match &block.content {
                BlockContent::Text { runs, .. } => {
                    Some(runs.iter().filter(|run| run.action.is_some()).map(|run| run.text.clone()).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect();
        let terms: Vec<_> = blocks
            .iter()
            .filter_map(|block| match &block.content {
                BlockContent::Term { term, .. } => Some(term.clone()),
                _ => None,
            })
            .collect();
        if !highlights.is_empty() && terms.is_empty() {
            failed = true;
        }
        let output = host.output.lock().unwrap().clone();
        let lower_output = output.to_lowercase();
        let unmet: Vec<_> = case
            .contains
            .iter()
            .filter(|phrase| !lower_output.contains(&phrase.to_lowercase()))
            .map(|phrase| format!("missing expected phrase: {phrase}"))
            .chain(
                case.excludes
                    .iter()
                    .filter(|phrase| lower_output.contains(&phrase.to_lowercase()))
                    .map(|phrase| format!("unexpected phrase: {phrase}")),
            )
            .collect();
        if !unmet.is_empty() {
            failed = true;
        }
        failures += usize::from(failed);
        println!(
            "{}",
            serde_json::json!({
                "input":case.transcript, "languages":case.languages, "output":output, "passed":!failed, "unmetExpectations":unmet,
                "highlights":highlights, "wordbookTerms":terms, "notice":notice,
                "invalidReplies":if notice.is_some() {host.replies.lock().unwrap().clone()} else {Vec::new()},
                "status":format!("{:?}", result.status), "elapsedMs":started.elapsed().as_millis(),
            })
        );
    }
    if failures > 0 {
        return Err(format!("{failures} replay cases failed").into());
    }
    Ok(())
}
