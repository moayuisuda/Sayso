import AVFoundation
import SaysoCore

/// 用系统语音朗读目标语言词句。多次调用自动排队依次朗读。
@MainActor
final class Pronouncer {
    private let synthesizer = AVSpeechSynthesizer()
    /// 语言与朗读语音的对应关系只在 core 定义一份。
    private let locales = Dictionary(uniqueKeysWithValues: supportedLanguages().map { ($0.language, $0.speechLocale) })

    func speak(_ text: String, in language: Language, rate: Float) {
        let utterance = AVSpeechUtterance(string: text)
        utterance.voice = AVSpeechSynthesisVoice(language: locales[language])
        utterance.rate = rate
        synthesizer.speak(utterance)
    }

    /// 打断当前及排队中的朗读，用于开始新一次录音时，避免朗读声被麦克风录进去。
    func stop() {
        synthesizer.stopSpeaking(at: .immediate)
    }
}
