import SaysoCore

/// 界面语言跟随用户的本地语言：本地语言为中文时显示中文，否则显示英文。
/// 本地语言来自 core 的设置，由 `CoreModel` 在设置变化时更新。
enum Localization {
    static var native: Language = .chinese
}

func L(_ zh: String, _ en: String) -> String {
    Localization.native == .chinese ? zh : en
}
