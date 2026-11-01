@preconcurrency import AVFoundation
import os

enum AudioRecorderError: Error, LocalizedError {
    case microphoneDenied
    case converterUnavailable

    var errorDescription: String? {
        switch self {
        case .microphoneDenied: return L("未授予麦克风权限", "Microphone permission not granted")
        case .converterUnavailable: return L("录音启动失败", "Failed to start recording")
        }
    }
}

/// 麦克风录音，输出 16kHz / 单声道 / Int16 小端 PCM，按 100ms 分块回调。
final class AudioRecorder: @unchecked Sendable {
    static let targetFormat = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: 16_000, channels: 1, interleaved: true)!
    /// 100ms 的 16kHz Int16 单声道数据。
    private static let chunkByteSize = 3_200

    private let engine = AVAudioEngine()
    private let lock = NSLock()
    private var pending = Data()
    private var onChunk: (@Sendable (Data) -> Void)?
    private var onLevel: (@Sendable (Float) -> Void)?
    /// 本次录音的送出统计，停止时写入日志：用于排查实时文字中途停止时，麦克风数据是否也已中断。
    private var startedAt = ContinuousClock.now
    private var chunkCount = 0
    private var lastChunkAt: ContinuousClock.Instant?
    private var configurationObserver: NSObjectProtocol?
    private let logger = Logger(subsystem: "com.sayso.app", category: "audio")

    func start(onChunk: @escaping @Sendable (Data) -> Void, onLevel: @escaping @Sendable (Float) -> Void) throws {
        guard AVCaptureDevice.authorizationStatus(for: .audio) == .authorized else {
            throw AudioRecorderError.microphoneDenied
        }
        let input = engine.inputNode
        let sourceFormat = input.outputFormat(forBus: 0)
        guard let converter = AVAudioConverter(from: sourceFormat, to: Self.targetFormat) else {
            throw AudioRecorderError.converterUnavailable
        }

        lock.withLock {
            pending = Data()
            self.onChunk = onChunk
            self.onLevel = onLevel
            startedAt = .now
            chunkCount = 0
            lastChunkAt = nil
        }
        // 输入设备切换等会让系统停止引擎，之后不再有音频回调。
        configurationObserver = NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange,
            object: engine,
            queue: nil
        ) { [logger, engine] _ in
            logger.error("audio engine configuration changed while recording, running=\(engine.isRunning)")
        }
        input.installTap(onBus: 0, bufferSize: 1_024, format: sourceFormat) { [weak self] buffer, _ in
            self?.process(buffer, converter: converter)
        }
        engine.prepare()
        do {
            try engine.start()
        } catch {
            input.removeTap(onBus: 0)
            throw error
        }
    }

    /// 停止录音并把不足一块的尾部数据一并交出。
    func stop() {
        if let configurationObserver { NotificationCenter.default.removeObserver(configurationObserver) }
        configurationObserver = nil
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        let (tail, callback, chunks, lastChunk, startedAt) = lock.withLock {
            defer {
                pending = Data()
                onChunk = nil
                onLevel = nil
            }
            return (pending, onChunk, chunkCount, lastChunkAt, self.startedAt)
        }
        if !tail.isEmpty { callback?(tail) }
        let silentFor = lastChunk.map { "\(ContinuousClock.now - $0)" } ?? "-"
        logger.notice("audio stopped after \(ContinuousClock.now - startedAt, privacy: .public): chunks=\(chunks) lastChunkAgo=\(silentFor, privacy: .public)")
    }

    private func process(_ buffer: AVAudioPCMBuffer, converter: AVAudioConverter) {
        let level = Self.level(of: buffer)
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * Self.targetFormat.sampleRate / buffer.format.sampleRate) + 1
        guard let converted = AVAudioPCMBuffer(pcmFormat: Self.targetFormat, frameCapacity: capacity) else { return }

        var consumed = false
        var error: NSError?
        let status = converter.convert(to: converted, error: &error) { _, outStatus in
            if consumed {
                outStatus.pointee = .noDataNow
                return nil
            }
            consumed = true
            outStatus.pointee = .haveData
            return buffer
        }
        guard status != .error, let samples = converted.int16ChannelData else { return }
        let data = Data(bytes: samples[0], count: Int(converted.frameLength) * MemoryLayout<Int16>.size)

        let (chunks, chunkCallback, levelCallback) = lock.withLock { () -> ([Data], (@Sendable (Data) -> Void)?, (@Sendable (Float) -> Void)?) in
            pending.append(data)
            var chunks: [Data] = []
            while pending.count >= Self.chunkByteSize {
                chunks.append(pending.prefix(Self.chunkByteSize))
                pending.removeFirst(Self.chunkByteSize)
            }
            if !chunks.isEmpty {
                chunkCount += chunks.count
                lastChunkAt = .now
            }
            return (chunks, onChunk, onLevel)
        }
        levelCallback?(level)
        chunks.forEach { chunkCallback?($0) }
    }

    /// 语音音量区间（实测内置麦克风：环境底噪约 -54...-45dB，正常说话约 -38...-22dB）。
    /// 人声很少接近 0dB，按满刻度映射会把轻声与大声挤在一起；下限压住底噪，上限对应大声说话。
    private static let quietDB: Float = -48
    private static let loudDB: Float = -20

    /// RMS 按语音音量区间映射到 0...1。
    private static func level(of buffer: AVAudioPCMBuffer) -> Float {
        guard let channel = buffer.floatChannelData?[0], buffer.frameLength > 0 else { return 0 }
        let frames = Int(buffer.frameLength)
        var sum: Float = 0
        for index in 0..<frames { sum += channel[index] * channel[index] }
        let db = 20 * log10(max(sqrt(sum / Float(frames)), 1e-7))
        return max(0, min(1, (db - quietDB) / (loudDB - quietDB)))
    }
}
