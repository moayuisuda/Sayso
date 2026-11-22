//! 集成测试将正式 helper 放到测试宿主旁，与应用安装目录保持相同布局。

pub fn install_shell_executable() {
    static INSTALLED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    INSTALLED.get_or_init(|| {
        let test = std::env::current_exe().unwrap();
        let directory = test.parent().unwrap();
        // Cargo 的测试产物可能位于更深的构建目录；沿输出目录向上定位同次构建的正式 helper。
        let source = directory
            .ancestors()
            .skip(1)
            .map(|parent| parent.join(sayso_shell::EXECUTABLE))
            .find(|path| path.is_file())
            .expect("run cargo test --workspace so the real sayso-shell executable is built");
        std::fs::copy(&source, directory.join(sayso_shell::EXECUTABLE))
            .expect("run cargo test --workspace so the real sayso-shell executable is built");
    });
}
