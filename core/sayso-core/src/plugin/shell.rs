//! Shell 生命周期与进程管理；解释器只在独立进程中运行，取消后不再执行任何脚本语句。

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use sayso_shell::Request;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::sync::{oneshot, watch, Semaphore};
use tokio::task::JoinHandle;

use crate::error::CoreError;

const IO_LIMIT: usize = 1024 * 1024;
static SLOTS: Semaphore = Semaphore::const_new(4);

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Options {
    pub cwd: Option<String>,
    env: HashMap<String, String>,
    stdin: String,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Output {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

/// 事件结束时停止仍在运行的脚本，包括插件未 await 的调用。
pub(super) struct Scope {
    cancel: watch::Sender<bool>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl Scope {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { cancel: watch::channel(false).0, workers: Mutex::default() })
    }

    pub fn cancel(&self) {
        self.cancel.send_replace(true);
    }

    pub async fn close(&self) {
        let workers = {
            let mut workers = self.workers.lock().unwrap();
            self.cancel();
            std::mem::take(&mut *workers)
        };
        for worker in workers {
            if let Err(error) = worker.await {
                log::error!("shell worker failed: {error}");
            }
        }
    }

    pub async fn run(
        &self,
        script: String,
        options: Options,
        cwd: PathBuf,
        deadline: Instant,
        mut surface: Option<watch::Receiver<()>>,
    ) -> Result<Output, CoreError> {
        if options.stdin.len() > IO_LIMIT {
            return Err(error("stdin exceeds 1 MiB"));
        }
        if options.env.iter().any(|(key, value)| key.is_empty() || key.contains(['=', '\0']) || value.contains('\0')) {
            return Err(error("invalid environment variable"));
        }
        let deadline = match options.timeout_ms {
            Some(milliseconds) if !(1..=60_000).contains(&milliseconds) => {
                return Err(error("timeoutMs must be between 1 and 60000"))
            }
            Some(milliseconds) => deadline.min(Instant::now() + Duration::from_millis(milliseconds)),
            None => deadline,
        };
        let mut cancelled = self.cancel.subscribe();
        let slot = tokio::select! {
            biased;
            _ = cancelled.wait_for(|cancelled| *cancelled) => return Err(error("cancelled")),
            _ = surface_closed(&mut surface) => return Err(error("cancelled")),
            _ = tokio::time::sleep_until(deadline.into()) => return Err(error("timed out")),
            slot = SLOTS.acquire() => slot.map_err(|err| error(err.to_string()))?,
        };
        let (send, receive) = oneshot::channel();
        {
            let mut workers = self.workers.lock().unwrap();
            if *self.cancel.borrow() {
                return Err(error("cancelled"));
            }
            workers.push(tokio::spawn(async move {
                let _slot = slot;
                let result = execute(script, options, cwd, deadline, cancelled, surface).await;
                let _ = send.send(result);
            }));
        }
        receive.await.map_err(|err| error(format!("worker failed: {err}")))?
    }
}

pub(super) struct CancelOnDrop(pub Arc<Scope>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

async fn execute(
    script: String,
    options: Options,
    cwd: PathBuf,
    deadline: Instant,
    mut cancelled: watch::Receiver<bool>,
    mut surface: Option<watch::Receiver<()>>,
) -> Result<Output, CoreError> {
    let outcome = tempfile::NamedTempFile::new()?;
    let request = Request { script, cwd, env: options.env, stdin: options.stdin, result: outcome.path().to_path_buf() };
    let request = serde_json::to_vec(&request)?;
    let executable = std::env::current_exe()?.with_file_name(sayso_shell::EXECUTABLE);
    let mut command = CommandWrap::with_new(executable, |command| {
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    });
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    command
        .wrap(process_wrap::tokio::JobObject)
        .wrap(process_wrap::tokio::CreationFlags(windows::Win32::System::Threading::CREATE_NO_WINDOW));
    let mut process = Process(command.spawn()?);
    let child = &mut process.0;
    let mut stdin = child.stdin().take().ok_or_else(|| error("stdin pipe missing"))?;
    let stdout = child.stdout().take().ok_or_else(|| error("stdout pipe missing"))?;
    let stderr = child.stderr().take().ok_or_else(|| error("stderr pipe missing"))?;
    let budget = Arc::new(AtomicUsize::new(IO_LIMIT));
    let result = {
        let completion = async {
            let write = async {
                stdin.write_all(&request).await?;
                stdin.shutdown().await?;
                drop(stdin);
                Ok::<_, CoreError>(())
            };
            let wait = async { child.wait().await.map_err(CoreError::from) };
            let (_, stdout, stderr, status) =
                tokio::try_join!(write, capture(stdout, budget.clone()), capture(stderr, budget), wait)?;
            if !status.success() {
                return Err(error(format!(
                    "execution process exited with {status}: {}",
                    String::from_utf8_lossy(&stderr)
                )));
            }
            let outcome: Result<i32, String> = serde_json::from_slice(&tokio::fs::read(outcome.path()).await?)?;
            Ok(Output {
                exit_code: outcome.map_err(error)?,
                stdout: String::from_utf8_lossy(&stdout).into_owned(),
                stderr: String::from_utf8_lossy(&stderr).into_owned(),
            })
        };
        tokio::select! {
            biased;
            _ = cancelled.wait_for(|cancelled| *cancelled) => Err(error("cancelled")),
            _ = surface_closed(&mut surface) => Err(error("cancelled")),
            _ = tokio::time::sleep_until(deadline.into()) => Err(error("timed out")),
            result = completion => result,
        }
    };
    // 组级回收包括外部程序的后代；Drop 也保留同样的语义，用于宿主任务被中止的情况。
    if result.is_err() && child.try_wait()?.is_none() {
        Box::into_pin(child.kill()).await?;
    }
    result
}

struct Process(Box<dyn ChildWrapper>);

impl Drop for Process {
    fn drop(&mut self) {
        // Unix 的 kill_on_drop 只结束直接子进程；这里始终调用组级 start_kill。
        let _ = self.0.start_kill();
    }
}

async fn capture(mut reader: impl AsyncRead + Unpin, budget: Arc<AtomicUsize>) -> Result<Vec<u8>, CoreError> {
    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let size = reader.read(&mut buffer).await?;
        if size == 0 {
            return Ok(output);
        }
        let available =
            budget.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| Some(left.saturating_sub(size))).unwrap();
        if size > available {
            return Err(error("combined stdout and stderr exceed 1 MiB"));
        }
        output.extend_from_slice(&buffer[..size]);
    }
}

async fn surface_closed(surface: &mut Option<watch::Receiver<()>>) {
    match surface {
        Some(surface) => {
            let _ = surface.changed().await;
        }
        None => std::future::pending().await,
    }
}

fn error(message: impl Into<String>) -> CoreError {
    CoreError::Shell(message.into())
}
