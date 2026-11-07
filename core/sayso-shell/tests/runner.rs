//! 直接运行正式 helper，验证执行结果不会占用脚本的输出或退出码。

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn execution_status_is_separate_from_script_output_and_exit_codes() {
    for (script, expected) in [("echo 内容; echo 问题 >&2; exit 125", Some(125)), ("echo (", None)] {
        let directory = tempfile::tempdir().unwrap();
        let outcome = directory.path().join("outcome.json");
        let request = sayso_shell::Request {
            script: script.into(),
            cwd: directory.path().to_path_buf(),
            env: Default::default(),
            stdin: String::new(),
            result: outcome.clone(),
        };
        let mut child = Command::new(env!("CARGO_BIN_EXE_sayso-shell"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&serde_json::to_vec(&request).unwrap()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let outcome: Result<i32, String> = serde_json::from_slice(&std::fs::read(outcome).unwrap()).unwrap();
        if let Some(exit) = expected {
            assert_eq!(outcome.unwrap(), exit);
            assert_eq!(String::from_utf8(output.stdout).unwrap(), "内容\n");
            assert_eq!(String::from_utf8(output.stderr).unwrap(), "问题\n");
        } else {
            assert!(outcome.is_err());
            assert!(output.stdout.is_empty() && output.stderr.is_empty());
        }
    }
}
