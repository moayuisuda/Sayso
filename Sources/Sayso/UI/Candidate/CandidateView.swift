import SwiftUI
import SaysoCore

/// 候选框：上方内容区，下方操作栏。
/// 内容区录音时显示实时识别、处理中显示待处理文本、结果按来源分组渲染块；操作栏显示当前状态与可用按键。
struct CandidateView: View {
    let model: CoreModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content
                .padding(.horizontal, 16)
                .padding(.vertical, 14)
            ActionBar { status } trailing: { actions }
        }
        .frame(minWidth: 340, maxWidth: 560, alignment: .leading)
        .background(.regularMaterial)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.panel, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Radius.panel, style: .continuous)
                .strokeBorder(Theme.panelStroke)
        )
        .overlay {
            if isRecording {
                FlowingBorder(level: model.audioLevel, cornerRadius: Theme.Radius.panel)
                    .transition(.opacity)
            }
        }
        .animation(.easeOut(duration: 0.25), value: isRecording)
        .fixedSize(horizontal: false, vertical: true)
        .tint(Theme.accent)
        .onHover { model.core.setHovering(hovering: $0) }
    }

    /// 内容超过这些高度后改为滚动：实时文本约 6 行，结果区约半屏以内。
    private enum MaxHeight {
        static let liveText: CGFloat = 132
        static let result: CGFloat = 360
    }

    @ViewBuilder
    private var content: some View {
        switch model.session {
        case .idle:
            EmptyView()
        case .recording(let partial, _):
            // 跟随最新识别的文字，正在说的内容始终可见。
            CappedScrollView(maxHeight: MaxHeight.liveText, followValue: partial) {
                Text(partial.isEmpty ? L("请说…", "Listening…") : partial)
                    .transcriptStyle()
                    .opacity(partial.isEmpty ? 0.65 : 1)
            }
        case .processing(let text):
            CappedScrollView(maxHeight: MaxHeight.liveText, followValue: text) {
                Text(text.isEmpty ? L("处理中…", "Working…") : text)
                    .transcriptStyle()
                    .opacity(text.isEmpty ? 0.65 : 1)
            }
        case .presenting(let surface):
            CappedScrollView(maxHeight: MaxHeight.result) {
                SurfaceContent(surface: surface, core: model.core)
            }
        }
    }

    /// 操作栏左侧：当前阶段。
    @ViewBuilder
    private var status: some View {
        if case .recording(_, let commands) = model.session {
            StatusLabel(([L("正在聆听", "Listening")] + commands).joined(separator: " · ")) {
                Image(systemName: "waveform")
                    .foregroundStyle(Theme.accentForeground)
                    .symbolEffect(.variableColor.iterative)
            }
        } else if isWorking {
            StatusLabel(L("处理中", "Working")) { ProgressView().controlSize(.mini) }
        } else if case .presenting = model.session {
            StatusLabel(L("言出", "Sayso")) { IconTile(systemImage: "character.bubble", size: 18) }
        }
    }

    private var isRecording: Bool {
        if case .recording = model.session { return true }
        return false
    }

    /// 识别、读取选区与扩展处理对用户是同一个阶段：同一个视图分支显示，阶段切换时状态不跳动。
    private var isWorking: Bool {
        switch model.session {
        case .processing: return true
        case .presenting(let surface): return surface.working
        case .idle, .recording: return false
        }
    }

    /// 操作栏右侧：录音与处理中提示按键，展示结果时提供关闭按钮。
    @ViewBuilder
    private var actions: some View {
        switch model.session {
        case .idle:
            EmptyView()
        case .recording:
            KeyHint(
                title: model.settings.hotkeyStyle == .hold ? L("松开结束", "Release to finish") : L("结束", "Finish"),
                keys: model.settings.triggerKey.keycaps
            )
            KeyHint(title: L("取消", "Cancel"), keys: ["esc"])
        case .processing:
            KeyHint(title: L("取消", "Cancel"), keys: ["esc"])
        case .presenting(let surface):
            Button(L("关闭", "Close")) { model.core.dismissSurface(surfaceId: surface.id) }
                .buttonStyle(.chip)
                .help(L("按 Enter 关闭候选框", "Press Enter to close the panel"))
        }
    }
}

/// 操作栏中的状态：图标 + 文字。
private struct StatusLabel<Icon: View>: View {
    let title: String
    @ViewBuilder let icon: Icon

    init(_ title: String, @ViewBuilder icon: () -> Icon) {
        self.title = title
        self.icon = icon()
    }

    var body: some View {
        HStack(spacing: 8) {
            icon
            Text(title)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.tail)
        }
    }
}

private struct SurfaceContent: View {
    let surface: SurfaceView
    let core: Core

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            ForEach(surface.sections, id: \.sourceId) { section in
                VStack(alignment: .leading, spacing: 8) {
                    if let title = section.title {
                        Text(title)
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(.secondary)
                    }
                    ForEach(section.blocks, id: \.id) { block in
                        BlockView(block: block, placement: .candidate) { action in
                            core.performCandidateAction(surfaceId: surface.id, sourceId: section.sourceId, blockId: block.id, action: action)
                        }
                    }
                }
            }
            // 扩展尚未输出任何块时先显示待处理内容（如选中的文本），进度在操作栏中显示。
            if surface.sections.isEmpty, surface.working {
                Text(surface.placeholder ?? L("处理中…", "Working…"))
                    .transcriptStyle()
                    .lineLimit(4)
            }
        }
    }
}
