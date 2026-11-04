import SwiftUI

/// 流程图：左列的节点汇入中间节点，再分流到右列的节点，节点之间用带箭头的线相连。
/// 两列各自纵向堆叠，节点再多也只增加高度，窄窗口下不会溢出。左右两列的节点用 `FlowNode` 声明，
/// 标题为空的一列不画。
struct FlowDiagram<Sources: View, Hub: View, Targets: View>: View {
    let sourcesTitle: String?
    let targetsTitle: String?
    @ViewBuilder let sources: Sources
    @ViewBuilder let hub: Hub
    @ViewBuilder let targets: Targets

    private static var hubWidth: CGFloat { 44 }
    private static var gap: CGFloat { 30 }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: Self.gap) {
                if let sourcesTitle { columnTitle(sourcesTitle) }
                Color.clear.frame(width: Self.hubWidth, height: 0)
                if let targetsTitle { columnTitle(targetsTitle) }
            }
            HStack(spacing: Self.gap) {
                if sourcesTitle != nil { column(sources) }
                hub
                    .frame(width: Self.hubWidth)
                    .anchorPreference(key: FlowAnchors.self, value: .bounds) { FlowAnchors.Value(hub: $0) }
                if targetsTitle != nil { column(targets) }
            }
            .backgroundPreferenceValue(FlowAnchors.self) { anchors in
                GeometryReader { proxy in
                    FlowLinks(links: anchors.links(in: proxy))
                }
            }
        }
    }

    private func columnTitle(_ title: String) -> some View {
        Text(title)
            .font(.system(size: 12, weight: .semibold))
            .foregroundStyle(.secondary)
            .padding(.leading, 4)
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func column(_ nodes: some View) -> some View {
        VStack(spacing: 10) { nodes }
            .frame(maxWidth: .infinity)
    }
}

enum FlowSide {
    case source, target
}

/// 流程图左右两列的节点。
struct FlowNode<Content: View>: View {
    let side: FlowSide
    @ViewBuilder let content: Content

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Radius.row, style: .continuous)
        VStack(alignment: .leading, spacing: 4) { content }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.cardFill, in: shape)
            .overlay(shape.strokeBorder(Theme.hairline))
            .anchorPreference(key: FlowAnchors.self, value: .bounds) { bounds in
                FlowAnchors.Value(nodes: [.init(side: side, bounds: bounds)])
            }
    }
}

/// 收集各节点的位置，换算成连线。
private struct FlowAnchors: PreferenceKey {
    struct Node {
        let side: FlowSide
        let bounds: Anchor<CGRect>
    }

    struct Value {
        var hub: Anchor<CGRect>?
        var nodes: [Node] = []

        func links(in proxy: GeometryProxy) -> [FlowLink] {
            guard let hub else { return [] }
            let center = proxy[hub]
            return nodes.map { node in
                let bounds = proxy[node.bounds]
                switch node.side {
                case .source:
                    let start = CGPoint(x: bounds.maxX, y: bounds.midY)
                    return FlowLink(start: start, end: CGPoint(x: center.minX, y: center.midY))
                case .target:
                    let end = CGPoint(x: bounds.minX, y: bounds.midY)
                    return FlowLink(start: CGPoint(x: center.maxX, y: center.midY), end: end)
                }
            }
        }
    }

    static let defaultValue = Value()

    static func reduce(value: inout Value, nextValue: () -> Value) {
        let next = nextValue()
        value.hub = value.hub ?? next.hub
        value.nodes += next.nodes
    }
}

private struct FlowLink {
    private static var arrowLength: CGFloat { 6 }

    let start: CGPoint
    let end: CGPoint

    /// 水平出入的三次曲线，终点留给箭头。
    var line: Path {
        let tip = CGPoint(x: end.x - Self.arrowLength, y: end.y)
        let middle = (start.x + tip.x) / 2
        var path = Path()
        path.move(to: start)
        path.addCurve(to: tip, control1: CGPoint(x: middle, y: start.y), control2: CGPoint(x: middle, y: tip.y))
        return path
    }

    var arrow: Path {
        var path = Path()
        path.move(to: end)
        path.addLine(to: CGPoint(x: end.x - Self.arrowLength, y: end.y - 3.5))
        path.addLine(to: CGPoint(x: end.x - Self.arrowLength, y: end.y + 3.5))
        path.closeSubpath()
        return path
    }
}

private struct FlowLinks: View {
    let links: [FlowLink]

    var body: some View {
        Canvas { context, _ in
            for link in links {
                context.stroke(link.line, with: .color(Theme.accentForeground), style: StrokeStyle(lineWidth: 1.25, lineCap: .round))
                context.fill(link.arrow, with: .color(Theme.accentForeground))
            }
        }
    }
}
