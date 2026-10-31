import os

/// 录音音量（0...1）的跨线程中转：音频线程直接写入最新值，界面按屏幕刷新率读取。
/// 音量每 100ms 才更新一次，逐次派发到主线程会让整棵视图随之重算；由界面主动拉取并在帧间平滑。
final class AudioLevel: Sendable {
    private let value = OSAllocatedUnfairLock<Float>(initialState: 0)

    var current: Float { value.withLock { $0 } }

    func set(_ level: Float) {
        value.withLock { $0 = level }
    }
}
