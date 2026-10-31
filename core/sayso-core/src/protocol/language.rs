use serde::{Deserialize, Serialize};

/// 书写系统。用于快速判断一段文本里是否可能含有本地语言片段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritingScript {
    Latin,
    Han,
    Japanese,
    Hangul,
    Cyrillic,
}

const HAN: &[(u32, u32)] = &[(0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xF900, 0xFAFF), (0x20000, 0x2FA1F)];

impl WritingScript {
    /// 该书写系统的特征字符范围。日文包含汉字，因此与 `Han` 存在交集。
    fn ranges(self) -> Vec<(u32, u32)> {
        match self {
            Self::Latin => vec![(0x41, 0x5A), (0x61, 0x7A), (0xC0, 0x24F)],
            Self::Han => HAN.to_vec(),
            Self::Japanese => [&[(0x3040, 0x309F), (0x30A0, 0x30FF)], HAN].concat(),
            Self::Hangul => vec![(0x1100, 0x11FF), (0x3130, 0x318F), (0xAC00, 0xD7AF)],
            Self::Cyrillic => vec![(0x400, 0x4FF)],
        }
    }

    fn contains(self, scalar: char) -> bool {
        let value = scalar as u32;
        self.ranges().iter().any(|&(low, high)| (low..=high).contains(&value))
    }

    fn overlaps(self, other: Self) -> bool {
        self.ranges()
            .iter()
            .any(|&(a_low, a_high)| other.ranges().iter().any(|&(b_low, b_high)| a_low <= b_high && b_low <= a_high))
    }
}

/// 学习场景支持的语言。序列化为 BCP-47 语言代码，作为持久化与扩展协议中的稳定值。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Enum)]
pub enum Language {
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh")]
    Chinese,
    #[serde(rename = "ja")]
    Japanese,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "ru")]
    Russian,
}

impl Language {
    pub const ALL: [Language; 8] = [
        Self::English,
        Self::Chinese,
        Self::Japanese,
        Self::Korean,
        Self::French,
        Self::German,
        Self::Spanish,
        Self::Russian,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Chinese => "zh",
            Self::Japanese => "ja",
            Self::Korean => "ko",
            Self::French => "fr",
            Self::German => "de",
            Self::Spanish => "es",
            Self::Russian => "ru",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|language| language.code() == code)
    }

    /// 写入 LLM 提示词时使用的英文名。
    pub fn english_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Chinese => "Chinese",
            Self::Japanese => "Japanese",
            Self::Korean => "Korean",
            Self::French => "French",
            Self::German => "German",
            Self::Spanish => "Spanish",
            Self::Russian => "Russian",
        }
    }

    /// 语言的自称，用于设置界面展示。
    pub fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Chinese => "中文",
            Self::Japanese => "日本語",
            Self::Korean => "한국어",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Spanish => "Español",
            Self::Russian => "Русский",
        }
    }

    /// 朗读时选择系统语音的区域代码。
    pub fn speech_locale(self) -> &'static str {
        match self {
            Self::English => "en-US",
            Self::Chinese => "zh-CN",
            Self::Japanese => "ja-JP",
            Self::Korean => "ko-KR",
            Self::French => "fr-FR",
            Self::German => "de-DE",
            Self::Spanish => "es-ES",
            Self::Russian => "ru-RU",
        }
    }

    pub fn script(self) -> WritingScript {
        match self {
            Self::English | Self::French | Self::German | Self::Spanish => WritingScript::Latin,
            Self::Chinese => WritingScript::Han,
            Self::Japanese => WritingScript::Japanese,
            Self::Korean => WritingScript::Hangul,
            Self::Russian => WritingScript::Cyrillic,
        }
    }
}

/// 一次输入使用的语言组合：目标语言（正在学的）与本地语言（母语）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct LanguagePair {
    pub target: Language,
    pub native: Language,
}

impl LanguagePair {
    /// 目标语言与本地语言必须不同，否则没有"夹杂"可言。
    pub fn is_valid(&self) -> bool {
        self.target != self.native
    }

    /// 判断文本是否可能夹杂本地语言。
    /// 两种语言书写系统不相交时（如英/中），可在本地精确判断，纯目标语言输入无需调用 LLM；
    /// 书写系统相交时（如法/英、日/中）无法靠字符区分，交由 LLM 判断。
    pub fn may_contain_native_span(&self, text: &str) -> bool {
        let native = self.native.script();
        if self.target.script().overlaps(native) {
            return true;
        }
        text.chars().any(|scalar| native.contains(scalar))
    }
}

impl Default for LanguagePair {
    fn default() -> Self {
        Self { target: Language::English, native: Language::Chinese }
    }
}

/// 设置界面列出的语言信息。
#[derive(Clone, Debug, uniffi::Record)]
pub struct LanguageInfo {
    pub language: Language,
    pub code: String,
    pub native_name: String,
    /// 平台语音合成使用的语言区域，如 en-US。
    pub speech_locale: String,
}

#[uniffi::export]
pub fn supported_languages() -> Vec<LanguageInfo> {
    Language::ALL
        .into_iter()
        .map(|language| LanguageInfo {
            language,
            code: language.code().into(),
            native_name: language.native_name().into(),
            speech_locale: language.speech_locale().into(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_scripts_always_ask_llm() {
        let french_learner = LanguagePair { target: Language::English, native: Language::French };
        assert!(french_learner.may_contain_native_span("plain english"));
        assert!(!LanguagePair::default().may_contain_native_span("plain english"));
        assert!(LanguagePair::default().may_contain_native_span("I have a 苹果"));
    }
}
