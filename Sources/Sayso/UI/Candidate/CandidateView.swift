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
            // 只动画透明度：窗口尺寸随内容立即变化，布局一旦参与动画，卡片会在已变大的窗口和阴影里慢慢伸缩，形成重影。
            // 流光常驻而不增删，否则淡出时停在旧尺寸，面板变高后会在卡片中间留下一条边线。
            FlowingBorder(level: model.audioLevel, cornerRadius: Theme.Radius.panel, isActive: isRecording)
                .animation(.easeOut(duration: 0.25)) { $0.opacity(isRecording ? 1 : 0) }
        }
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
        case .recording(let partial, _, _):
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
        if case .recording(_, let command, _) = model.session {
            // 图标保持静态，录音中的动态只由流光承担（见 FlowingBorder）。窗口图层树内持续播放的动画
            // （如 symbolEffect）会让每帧提交同步等待窗口服务器，主队列长期得不到调度，
            // core 推送的会话状态随之积压，候选框卡在录音画面且无法关闭。
            StatusLabel("\(L("正在聆听", "Listening")) · \(command)") {
                Image(systemName: "waveform")
                    .foregroundStyle(Theme.accentForeground)
            }
        } else if isWorking {
            StatusLabel(L("处理中", "Working")) { ProgressView().controlSize(.mini) }
        } else if case .presenting = model.session {
            StatusLabel("Sayso") { AppIconView(size: 18) }
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

    /// 操作栏右侧：录音与处理中提示按键；展示结果时为针对整次翻译的扩展按钮与关闭按钮。
    @ViewBuilder
    private var actions: some View {
        switch model.session {
        case .idle:
            EmptyView()
        case .recording(_, _, let shortcut):
            KeyHint(
                title: model.settings.hotkeyStyle == .hold ? L("松开结束", "Release to finish") : L("结束", "Finish"),
                keys: shortcut.keycaps
            )
            KeyHint(title: L("取消", "Cancel"), keys: ["esc"])
        case .processing:
            KeyHint(title: L("取消", "Cancel"), keys: ["esc"])
        case .presenting(let surface):
            // 附加在整句卡片上的按钮在关闭左侧，放不下时向上换行、靠右对齐；优先占用状态以外的宽度。
            FlowLayout(spacing: 6, alignment: .trailing) {
                ForEach(Array(surface.footer.enumerated()), id: \.offset) { _, attachment in
                    ForEach(Array(attachment.actions.enumerated()), id: \.offset) { _, action in
                        ActionButton(action: action) {
                            model.core.performCandidateAction(surfaceId: surface.id, sourceId: attachment.sourceId, blockId: attachment.blockRef, action: action)
                        }
                    }
                }
                Button(L("关闭", "Close")) { model.core.dismissSurface(surfaceId: surface.id) }
                    .buttonStyle(.chip)
                    .help(L("按 Enter 关闭候选框", "Press Enter to close the panel"))
            }
            .layoutPriority(1)
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
                    ForEach(section.blocks, id: \.block.id) { item in
                        BlockView(block: item.block, placement: .candidate, attachments: item.attachments) { action in
                            core.performCandidateAction(surfaceId: surface.id, sourceId: section.sourceId, blockId: item.block.id, action: action)
                        } performAttached: { attachment, action in
                            // 附加的按钮回调给附加方，块引用原样交回。
                            core.performCandidateAction(surfaceId: surface.id, sourceId: attachment.sourceId, blockId: attachment.blockRef, action: action)
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
