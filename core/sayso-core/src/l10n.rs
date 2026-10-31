//! core 产生的提示与错误跟随本地语言：本地语言为中文时输出中文，否则输出英文。

use std::sync::atomic::{AtomicBool, Ordering};

use crate::protocol::Language;

static CHINESE: AtomicBool = AtomicBool::new(true);

/// 设置变化时同步本地语言。
pub fn set_native_language(language: Language) {
    CHINESE.store(language == Language::Chinese, Ordering::Relaxed);
}

pub fn tr(zh: &str, en: &str) -> String {
    if CHINESE.load(Ordering::Relaxed) { zh } else { en }.to_string()
}
