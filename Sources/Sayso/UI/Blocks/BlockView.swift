import SwiftUI
import SaysoCore

/// 渲染一个块。候选框与扩展页签共用同一组件，保证内置内容与扩展内容外观一致。
struct BlockView: View {
    enum Placement {
        case candidate
        case tab
    }

    let block: Block
    let placement: Placement
    /// 其他扩展附加在块上的按钮（仅候选框），排在块自身的操作之后。
    var attachments: [Attachment] = []
    let perform: (BlockAction) -> Void
    /// 点击附加的按钮，由附加方处理。
    var performAttached: (Attachment, BlockAction) -> Void = { _, _ in }

    var body: some View {
        // 词条与状态是单行内容，按钮跟在同一行末尾，放不下时换行（页签列表中靠右对齐）；其他块的按钮另起一行，同样可换行。
        if isSingleLine, placement == .tab {
            HStack(spacing: 8) {
                content
                Spacer(minLength: 8)
                HStack(spacing: 6) { buttons }
            }
        } else if isSingleLine {
            FlowLayout(spacing: 6) {
                content
                buttons
            }
        } else {
            VStack(alignment: .leading, spacing: 6) {
                content
                if hasButtons {
                    FlowLayout(spacing: 6) { buttons }
                }
            }
        }
    }

    private var isSingleLine: Bool {
        switch block.content {
        case .term, .status: return true
        case .text, .card: return false
        }
    }

    private var hasButtons: Bool {
        !buttonActions.isEmpty || !attachments.isEmpty
    }

    /// 块自身的操作在前，其他扩展附加的按钮在后；逐个作为布局的子视图，由外层决定排列与换行。
    @ViewBuilder
    private var buttons: some View {
        ForEach(Array(buttonActions.enumerated()), id: \.offset) { _, action in
            ActionButton(action: action) { perform(action) }
        }
        ForEach(attachments, id: \.sourceId) { attachment in
            ForEach(Array(attachment.actions.enumerated()), id: \.offset) { _, action in
                ActionButton(action: action) { performAttached(attachment, action) }
            }
        }
    }

    @ViewBuilder
    private var content: some View {
        switch block.content {
        case .text(let runs, let role):
            TextBlockView(block: block, runs: runs, role: role, perform: perform)
        case .term(let term, let gloss, let language, let detail):
            TermBlockView(term: term, gloss: gloss, detail: detail) {
                perform(BlockAction(id: nil, label: nil, disabled: false, kind: .speak(text: term, language: language)))
            }
        case .card(let title, let subtitle, let body):
            VStack(alignment: .leading, spacing: 3) {
                Text(title).font(.system(size: 13, weight: .semibold))
                if let subtitle = subtitle.nonEmpty {
                    Text(subtitle).font(.system(size: 12)).foregroundStyle(.secondary)
                }
                if let body = body.nonEmpty {
                    Text(body).font(.system(size: 12))
                }
            }
        case .status(let message, let icon, let tone):
            Label(message, systemImage: icon.nonEmpty ?? tone.defaultIcon)
                .font(.system(size: 12))
                .foregroundStyle(tone.color)
        }
    }

    /// 文本片段已引用的操作通过点击片段触发，其余操作显示为按钮。删除只在页签中可用。
    private var buttonActions: [BlockAction] {
        let inline: Set<String> = {
            guard case .text(let runs, _) = block.content else { return [] }
            return Set(runs.compactMap(\.action))
        }()
        return block.actions.filter { action in
            if let id = action.id, inline.contains(id) { return false }
            if case .delete = action.kind { return placement == .tab }
            return true
        }
    }
}

/// 富文本块：高亮片段为主题色底色与文字，带操作的片段可点击。
private struct TextBlockView: View {
    private static let scheme = "sayso-action"

    let block: Block
    let runs: [TextRun]
    let role: TextRole?
    let perform: (BlockAction) -> Void

    var body: some View {
        Text(attributed)
            .transcriptStyle(secondary: role == .secondary)
            .fixedSize(horizontal: false, vertical: true)
            .environment(\.openURL, OpenURLAction { url in
                if url.scheme == Self.scheme, let id = url.host, let action = block.action(withID: id) {
                    perform(action)
                }
                return .handled
            })
    }

    private var attributed: AttributedString {
        runs.reduce(into: AttributedString()) { result, run in
            var piece = AttributedString(run.text)
            switch run.style ?? .normal {
            case .normal:
                break
            case .highlight:
                piece.backgroundColor = Theme.highlightFill
                piece.foregroundColor = Theme.accentForeground
            case .muted:
                piece.foregroundColor = .secondary
            }
            if let id = run.action, block.action(withID: id) != nil, let url = URL(string: "\(Self.scheme)://\(id)") {
                piece.link = url
            }
            result += piece
        }
    }
}

/// 词条：朗读按钮 + 词句 + 释义，可带一行补充信息。
private struct TermBlockView: View {
    let term: String
    let gloss: String?
    let detail: String?
    let speak: () -> Void

    var body: some View {
        Button(action: speak) {
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 5) {
                    Image(systemName: "speaker.wave.2.fill")
                        .font(.system(size: 9))
                        .foregroundStyle(Theme.accentForeground)
                    Text(term).fontWeight(.semibold)
                    if let gloss = gloss.nonEmpty {
                        Text(gloss).foregroundStyle(.secondary)
                    }
                }
                .font(.system(size: 12))
                if let detail = detail.nonEmpty {
                    Text(detail).font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(2)
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(Theme.highlightFill, in: RoundedRectangle(cornerRadius: Theme.Radius.control, style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(L("朗读", "Play"))
    }
}

/// 块操作按钮，块与候选框底栏共用。
struct ActionButton: View {
    let action: BlockAction
    let run: () -> Void

    var body: some View {
        Button(role: isDestructive ? .destructive : nil, action: run) {
            Label(title, systemImage: icon)
        }
        .buttonStyle(.chip)
        .disabled(action.disabled)
    }

    private var isDestructive: Bool {
        if case .delete = action.kind { return true }
        return false
    }

    private var title: String {
        if let label = action.label.nonEmpty { return label }
        switch action.kind {
        case .speak: return L("朗读", "Play")
        case .copy: return L("复制", "Copy")
        case .open: return L("打开", "Open")
        case .insert: return L("输入", "Insert")
        case .invoke: return L("执行", "Run")
        case .delete: return L("删除", "Delete")
        }
    }

    private var icon: String {
        // 不可点击的按钮用于展示已完成的状态。
        if action.disabled { return "checkmark" }
        switch action.kind {
        case .speak: return "speaker.wave.2.fill"
        case .copy: return "doc.on.doc"
        case .open: return "safari"
        case .insert: return "text.insert"
        case .invoke: return "plus"
        case .delete: return "trash"
        }
    }
}

private extension Block {
    func action(withID id: String) -> BlockAction? {
        actions.first { $0.id == id }
    }
}

private extension Optional where Wrapped == Tone {
    var color: Color {
        switch self ?? .info {
        case .info: return .secondary
        case .success: return .green
        case .warning: return .orange
        case .error: return .red
        }
    }

    var defaultIcon: String {
        switch self ?? .info {
        case .info: return "info.circle"
        case .success: return "checkmark.circle.fill"
        case .warning: return "exclamationmark.circle"
        case .error: return "exclamationmark.triangle.fill"
        }
    }
}

private extension Optional where Wrapped == String {
    /// 模板替换后缺失的字段为空串，与 nil 一样不显示。
    var nonEmpty: String? {
        guard let self, !self.isEmpty else { return nil }
        return self
    }
}
