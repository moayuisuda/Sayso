//! 每次调用独立执行；父进程负责限时、输出限额和整组进程的回收。

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Seek, Write};

use deno_task_shell::{KillSignal, ShellPipeReader, ShellPipeWriter, ShellState};
use sayso_shell::Request;

fn main() {
    if let Err(error) = run() {
        eprintln!("sayso-shell: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let request: Request = serde_json::from_reader(std::io::stdin().lock())?;
    let outcome = execute(&request).map_err(|error| error.to_string());
    std::fs::write(&request.result, serde_json::to_vec(&outcome)?)?;
    Ok(())
}

fn execute(request: &Request) -> Result<i32, Box<dyn std::error::Error>> {
    let commands = deno_task_shell::parser::parse(&request.script)?;
    let cwd = std::fs::canonicalize(&request.cwd)?;
    if !cwd.is_dir() {
        return Err("cwd must be a directory".into());
    }
    let mut env: HashMap<_, _> = std::env::vars_os().map(|(key, value)| (env_name(key), value)).collect();
    env.extend(request.env.iter().map(|(key, value)| (env_name(key.into()), value.into())));
    let state = ShellState::new(env, cwd, HashMap::new(), KillSignal::default());
    // 请求经父进程 stdin 传输；脚本得到独立文本输入，不继承控制通道。
    let mut input = tempfile::tempfile()?;
    input.write_all(request.stdin.as_bytes())?;
    input.rewind()?;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    Ok(runtime.block_on(tokio::task::LocalSet::new().run_until(deno_task_shell::execute_with_pipes(
        commands,
        state,
        ShellPipeReader::from_std(input),
        ShellPipeWriter::stdout(),
        ShellPipeWriter::stderr(),
    ))))
}

fn env_name(name: OsString) -> OsString {
    if cfg!(windows) {
        name.to_ascii_uppercase()
    } else {
        name
    }
}
