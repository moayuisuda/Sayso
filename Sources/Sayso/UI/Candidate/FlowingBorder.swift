import SwiftUI

/// 录音时面板边缘的流光，由显示器刷新驱动（TimelineView）：每帧读取最新音量并平滑，音量本身不触发视图更新。
struct FlowingBorder: View {
    let level: AudioLevel
    let cornerRadius: CGFloat

    @State private var motion = FlowMotion()
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        TimelineView(.animation) { timeline in
            let frame = motion.advance(to: timeline.date, target: level.current, flowing: !reduceMotion)
            FlowingGlow(level: CGFloat(frame.level), flow: frame.flow, cornerRadius: cornerRadius)
        }
        .allowsHitTesting(false)
    }
}

/// 某一时刻的流光画面：几层柔和光晕沿边缘以不同速度、方向流动，色标位置与亮度各自起伏，
/// 叠在一起形成不断聚散的光团。说话越响，光晕越宽越亮。
struct FlowingGlow: View {
    /// 平滑后的音量，0...1。
    let level: CGFloat
    /// 流动进度：随时间累积，说话时累积更快。
    let flow: Double
    let cornerRadius: CGFloat

    /// 由内到外：清晰的边线、中层光晕、外层大范围柔光。反向的一层让光团交汇而不是整体旋转。
    /// 安静时只留细边与淡光，越往外的层随音量变化越大，大声说话时光晕铺满边缘。
    private static let layers = [
        GlowLayer(lineWidth: 1.5...3, blur: 0...1.5, opacity: 0.55...1, speed: 1, seed: 0),
        GlowLayer(lineWidth: 3...16, blur: 3...10, opacity: 0.3...1, speed: -0.7, seed: 2.1),
        GlowLayer(lineWidth: 4...40, blur: 6...24, opacity: 0...0.95, speed: 0.45, seed: 4.3),
    ]

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
        ZStack {
            ForEach(Self.layers.indices, id: \.self) { index in
                let layer = Self.layers[index]
                shape
                    .stroke(layer.gradient(flow: flow), lineWidth: layer.lineWidth.at(level))
                    .blur(radius: layer.blur.at(level))
                    .opacity(layer.opacity.at(level))
            }
        }
        // 描边一半落在面板外被裁掉，光只向内晕开。
        .clipShape(shape)
        // 必须离屏合成为独立图层：逐帧动画若直接画在窗口图层树中，每帧提交都会同步等待窗口服务器，
        // 主队列长期得不到调度，core 推送的会话状态积压，候选框卡在录音画面。
        .drawingGroup()
    }
}

/// 一层光晕：尺寸随音量在区间内变化，色标在边缘上随流动进度游走。
private struct GlowLayer {
    let lineWidth: ClosedRange<CGFloat>
    let blur: ClosedRange<CGFloat>
    let opacity: ClosedRange<CGFloat>
    /// 相对流速（圈/单位进度），负值反向流动。
    let speed: Double
    /// 区分各层的相位，避免几层同步起伏。
    let seed: Double

    /// 色标在各自等分位置附近摆动（幅度小于间距，顺序不变），亮度各自起伏；首尾同色以便环绕无接缝。
    func gradient(flow: Double) -> AngularGradient {
        let colors = Theme.glow
        let count = Double(colors.count)
        func brightness(_ index: Int) -> Double {
            0.45 + 0.55 * (0.5 + 0.5 * sin(flow * (7 + 1.1 * Double(index)) + seed * 1.9 + Double(index) * 2.7))
        }
        var stops = [Gradient.Stop(color: colors[0].opacity(brightness(0)), location: 0)]
        for index in 1..<colors.count {
            let drift = 0.35 * sin(flow * (5 + 0.9 * Double(index)) + seed + Double(index) * 1.3)
            stops.append(.init(color: colors[index].opacity(brightness(index)), location: (Double(index) + drift) / count))
        }
        stops.append(.init(color: colors[0].opacity(brightness(0)), location: 1))
        return AngularGradient(stops: stops, center: .center, angle: .degrees((flow * speed + seed / 6) * 360))
    }
}

private extension ClosedRange where Bound == CGFloat {
    /// 按 0...1 的比例取区间内的值。
    func at(_ fraction: CGFloat) -> CGFloat {
        lowerBound + (upperBound - lowerBound) * fraction
    }
}

/// 帧间状态：平滑后的音量与流动进度。按真实帧间隔积分，与刷新率无关。
private final class FlowMotion {
    struct Frame {
        let level: Float
        let flow: Double
    }

    /// 音量上升快、回落稍慢：开口说话立即亮起，随音节起伏而不闪烁。
    private static let attack: Double = 0.05
    private static let release: Double = 0.2
    /// 流动速度（进度/秒）：安静时缓慢流淌，说话越响流得越快。
    private static let idleSpeed: Double = 0.1
    private static let speakingSpeed: Double = 0.5

    private var level: Float = 0
    private var flow: Double = 0
    private var lastDate: Date?

    func advance(to date: Date, target: Float, flowing: Bool) -> Frame {
        // 首帧或长时间未绘制（面板隐藏后重新出现）时不累积跳变。
        let dt = min(lastDate.map { date.timeIntervalSince($0) } ?? 0, 0.1)
        lastDate = date
        let tau = target > level ? Self.attack : Self.release
        level += (target - level) * Float(1 - exp(-dt / tau))
        if flowing {
            flow += (Self.idleSpeed + (Self.speakingSpeed - Self.idleSpeed) * Double(level)) * dt
        }
        return Frame(level: level, flow: flow)
    }
}
