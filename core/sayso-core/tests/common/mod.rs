//! 集成测试将正式 helper 放到测试宿主旁，与应用安装目录保持相同布局。

pub fn install_shell_executable() {
    static INSTALLED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    INSTALLED.get_or_init(|| {
        let test = std::env::current_exe().unwrap();
        let directory = test.parent().unwrap();
        let source = directory.parent().unwrap().join(sayso_shell::EXECUTABLE);
        std::fs::copy(&source, directory.join(sayso_shell::EXECUTABLE))
            .expect("run cargo test --workspace so the real sayso-shell executable is built");
    });
}
