//! 在内嵌 QuickJS 中执行一次事件处理：每个事件一个新的运行时与上下文，互不共享状态。

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rquickjs::loader::{ImportAttributes, Resolver, ScriptLoader};
use rquickjs::prelude::Async;
use rquickjs::{AsyncContext, AsyncRuntime, Ctx, Exception, Function, Module, Object, Value};

use super::{Host, Instruction, Plugin, PluginSource, RunResult, RunStatus};
use crate::error::CoreError;
use crate::protocol::{Capability, Delivery, Event, EventType, LanguagePair, Manifest};

const PRELUDE: &str = include_str!("prelude.js");
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const STACK_LIMIT: usize = 1024 * 1024;
const LOG_LIMIT: usize = 4096;

/// 执行插件对一个事件的处理，施加超时、错误隔离与能力校验。
pub async fn run(plugin: &Plugin, event: &Event, host: Arc<dyn Host>) -> RunResult {
    let invocation = Arc::new(Invocation {
        manifest: plugin.manifest.clone(),
        event_type: event.event_type(),
        languages: event.languages,
        directory: plugin.directory().cloned(),
        host,
        warnings: Mutex::default(),
        log: Mutex::default(),
    });
    let timeout = plugin.manifest.timeout();
    let status =
        match tokio::time::timeout(timeout, execute(plugin, event, invocation.clone(), Instant::now() + timeout)).await
        {
            Ok(Ok(())) => RunStatus::Succeeded,
            Ok(Err(Failure::TimedOut)) | Err(_) => RunStatus::TimedOut,
            Ok(Err(Failure::Error(message))) => RunStatus::Failed { message },
        };
    let warnings = invocation.warnings.lock().unwrap().clone();
    let log = invocation.log.lock().unwrap().clone();
    RunResult { extension_id: plugin.manifest.id.clone(), status, warnings, log }
}

enum Failure {
    TimedOut,
    Error(String),
}

async fn execute(
    plugin: &Plugin,
    event: &Event,
    invocation: Arc<Invocation>,
    deadline: Instant,
) -> Result<(), Failure> {
    let runtime_error = |error: rquickjs::Error| Failure::Error(error.to_string());
    let runtime = AsyncRuntime::new().map_err(runtime_error)?;
    runtime.set_memory_limit(MEMORY_LIMIT).await;
    runtime.set_max_stack_size(STACK_LIMIT).await;
    // 同步死循环不会让出执行权，tokio 的超时无法打断，由解释器的中断检查兜住。
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline))).await;

    let (module_name, source) = match &plugin.source {
        PluginSource::BuiltIn { main } => (format!("{}/main.js", plugin.manifest.id), main.to_string()),
        PluginSource::Installed { directory } => {
            let main = directory.join(plugin.manifest.main.as_deref().unwrap_or_default());
            let source = std::fs::read_to_string(&main)
                .map_err(|error| Failure::Error(format!("{}: {error}", main.display())))?;
            runtime.set_loader(PluginResolver { root: directory.clone() }, ScriptLoader::default()).await;
            (main.to_string_lossy().into_owned(), source)
        }
    };
    let context = AsyncContext::full(&runtime).await.map_err(runtime_error)?;
    let handler = event.event_type().name();
    let event_json = event.to_json().to_string();

    let outcome: Result<(), String> = context
        .async_with(async |ctx| {
            let result = async {
                install(&ctx, invocation.clone())?;
                ctx.eval::<(), _>(PRELUDE)?;
                let (module, loaded) = Module::declare(ctx.clone(), module_name, source)?.eval()?;
                loaded.into_future::<()>().await?;
                let Ok(function) = module.get::<_, Function>(handler) else {
                    invocation.warn(format!("main module exports no `{handler}` handler"));
                    return Ok(());
                };
                let returned: Value = function.call((ctx.json_parse(event_json)?,))?;
                if let Some(promise) = returned.into_promise() {
                    promise.into_future::<()>().await?;
                }
                Ok::<_, rquickjs::Error>(())
            }
            .await;
            result.map_err(|error| describe(&ctx, error, &invocation))
        })
        .await;
    match outcome {
        Ok(()) => Ok(()),
        Err(_) if Instant::now() >= deadline => Err(Failure::TimedOut),
        Err(message) => Err(Failure::Error(message)),
    }
}

/// 只解析扩展目录内的相对导入（`./`、`../`），解析结果不能离开扩展目录。
struct PluginResolver {
    root: PathBuf,
}

impl Resolver for PluginResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        let failure = || rquickjs::Error::new_resolving(base, name);
        if !name.starts_with("./") && !name.starts_with("../") {
            return Err(failure());
        }
        let parent = Path::new(base).parent().ok_or_else(failure)?;
        // 词法规范化：先消去 `.` 与 `..`，再判断是否仍在扩展目录内。
        let mut path = PathBuf::new();
        for component in parent.join(name).components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    path.pop();
                }
                other => path.push(other),
            }
        }
        if !path.starts_with(&self.root) || !path.is_file() {
            return Err(failure());
        }
        Ok(path.to_string_lossy().into_owned())
    }
}

/// 把 JS 异常转为错误文本；调用栈记入日志，便于扩展作者排查。
fn describe(ctx: &Ctx<'_>, error: rquickjs::Error, invocation: &Invocation) -> String {
    if !matches!(error, rquickjs::Error::Exception) {
        return error.to_string();
    }
    let thrown = ctx.catch();
    if let Some(exception) = thrown.as_exception() {
        if let Some(stack) = exception.stack() {
            invocation.append_log(&stack);
        }
        return exception.message().unwrap_or_else(|| "exception".into());
    }
    ctx.json_stringify(thrown)
        .ok()
        .flatten()
        .and_then(|text| text.to_string().ok())
        .unwrap_or_else(|| "exception".into())
}

/// 安装 prelude 使用的原生函数，全部只收发字符串。
fn install<'js>(ctx: &Ctx<'js>, invocation: Arc<Invocation>) -> rquickjs::Result<()> {
    let native = Object::new(ctx.clone())?;
    native.set("languages", languages_json(invocation.languages))?;

    let emitter = invocation.clone();
    native.set("emit", Function::new(ctx.clone(), move |json: String| emitter.emit(&json))?)?;

    let logger = invocation.clone();
    native.set("log", Function::new(ctx.clone(), move |line: String| logger.append_log(&line))?)?;

    let languages = invocation.languages;
    native.set(
        "mayContainNative",
        Function::new(ctx.clone(), move |text: String| languages.may_contain_native_span(&text))?,
    )?;

    let reader = invocation.clone();
    native.set(
        "records",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, collection: String| {
            reader.records(&collection).map_err(|error| throw(&ctx, error))
        })?,
    )?;

    let files = invocation.clone();
    native.set(
        "fs",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, op: String, path: String, text: String| {
            files.fs(&op, &path, &text).map_err(|error| throw(&ctx, error))
        })?,
    )?;

    let completer = invocation.clone();
    native.set(
        "llm",
        Function::new(
            ctx.clone(),
            Async(move |ctx: Ctx<'js>, system: String, user: String| {
                let invocation = completer.clone();
                async move { invocation.complete(system, user).await.map_err(|error| throw(&ctx, error)) }
            }),
        )?,
    )?;

    let inserter = invocation;
    native.set(
        "insert",
        Function::new(
            ctx.clone(),
            Async(move |ctx: Ctx<'js>, text: String| {
                let invocation = inserter.clone();
                async move { invocation.insert(text).await.map_err(|error| throw(&ctx, error)) }
            }),
        )?,
    )?;

    ctx.globals().set("__sayso", native)
}

fn throw(ctx: &Ctx<'_>, error: CoreError) -> rquickjs::Error {
    Exception::throw_message(ctx, &error.to_string())
}

fn languages_json(languages: LanguagePair) -> String {
    let describe = |language: crate::protocol::Language| serde_json::json!({ "code": language.code(), "name": language.english_name() });
    serde_json::json!({ "target": describe(languages.target), "native": describe(languages.native) }).to_string()
}

/// 一次事件处理的状态：能力校验、警告与日志。
struct Invocation {
    manifest: Manifest,
    event_type: EventType,
    languages: LanguagePair,
    directory: Option<PathBuf>,
    host: Arc<dyn Host>,
    warnings: Mutex<Vec<String>>,
    log: Mutex<String>,
}

impl Invocation {
    fn warn(&self, message: String) {
        self.warnings.lock().unwrap().push(message);
    }

    fn append_log(&self, line: &str) {
        let mut log = self.log.lock().unwrap();
        log.push_str(line);
        log.push('\n');
        if log.len() > LOG_LIMIT {
            let mut cut = log.len() - LOG_LIMIT;
            while !log.is_char_boundary(cut) {
                cut += 1;
            }
            log.drain(..cut);
        }
    }

    fn require(&self, capability: Capability) -> Result<(), CoreError> {
        if self.manifest.grants(capability) {
            Ok(())
        } else {
            Err(CoreError::CapabilityNotGranted(capability))
        }
    }

    /// 指令出错不中断插件：无法解析、未声明能力或执行失败都记为警告。
    fn emit(&self, json: &str) {
        let instruction = match Instruction::parse(json) {
            Ok(instruction) => instruction,
            Err(error) => return self.warn(error.to_string()),
        };
        if let Some(missing) = instruction.required_capabilities().into_iter().find(|c| !self.manifest.grants(*c)) {
            return self.warn(CoreError::CapabilityNotGranted(missing).to_string());
        }
        // 广播只在处理命令时允许：处理广播事件时不能再广播，避免循环。
        if matches!(instruction, Instruction::PublishTranslation { .. }) && self.event_type != EventType::Command {
            return self.warn("publish is only allowed while handling the command event".into());
        }
        if let Err(error) = self.host.apply(instruction) {
            self.warn(error.to_string());
        }
    }

    async fn complete(&self, system: String, user: String) -> Result<String, CoreError> {
        self.require(Capability::Llm)?;
        self.host.complete(system, user).await
    }

    /// 写入其他应用必须由用户触发：只能在处理命令时使用。
    async fn insert(&self, text: String) -> Result<String, CoreError> {
        self.require(Capability::Insert)?;
        if self.event_type != EventType::Command {
            return Err(CoreError::InsertOutsideCommand);
        }
        Ok(match self.host.insert(text).await? {
            Delivery::Inserted => "inserted",
            Delivery::CopiedToClipboard => "copiedToClipboard",
        }
        .into())
    }

    fn records(&self, collection: &str) -> Result<String, CoreError> {
        self.require(Capability::Store)?;
        Ok(serde_json::to_string(&self.host.records(collection)?)?)
    }

    fn fs(&self, op: &str, path: &str, text: &str) -> Result<String, CoreError> {
        self.require(Capability::Fs)?;
        let path = self.resolve(path)?;
        match op {
            "read" => Ok(std::fs::read_to_string(&path)?),
            "write" => Ok(std::fs::write(&path, text).map(|_| String::new())?),
            "append" => {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
                file.write_all(text.as_bytes())?;
                Ok(String::new())
            }
            "exists" => Ok(path.exists().to_string()),
            "mkdir" => Ok(std::fs::create_dir_all(&path).map(|_| String::new())?),
            other => Err(CoreError::InvalidInstruction(format!("unknown fs operation {other}"))),
        }
    }

    /// `~` 展开为用户主目录；相对路径相对扩展目录。
    fn resolve(&self, path: &str) -> Result<PathBuf, CoreError> {
        if let Some(rest) = path.strip_prefix("~/").or(if path == "~" { Some("") } else { None }) {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
            return home
                .map(|home| Path::new(&home).join(rest))
                .ok_or_else(|| CoreError::Io("home directory unknown".into()));
        }
        let path = Path::new(path);
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }
        self.directory
            .as_ref()
            .map(|directory| directory.join(path))
            .ok_or_else(|| CoreError::Io("relative paths need an extension directory".into()))
    }
}
