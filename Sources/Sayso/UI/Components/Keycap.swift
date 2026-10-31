import SwiftUI

/// 键帽：显示单个按键，用于快捷键展示与操作提示。
struct Keycap: View {
    let label: String

    var body: some View {
        Text(label)
            .font(.system(size: 11, weight: .medium, design: .rounded))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 5)
            .frame(minWidth: 20, minHeight: 20)
            .background(Theme.keycapFill, in: RoundedRectangle(cornerRadius: Theme.Radius.keycap, style: .continuous))
    }
}

/// 按顺序排列的一组键帽，表示一个组合键。
struct KeycapRow: View {
    let keys: [String]

    var body: some View {
        HStack(spacing: 3) {
            ForEach(Array(keys.enumerated()), id: \.offset) { _, key in
                Keycap(label: key)
            }
        }
    }
}

/// 操作提示：说明文字 + 组合键，例如「取消 esc」。
struct KeyHint: View {
    let title: String
    let keys: [String]

    var body: some View {
        HStack(spacing: 6) {
            Text(title).foregroundStyle(.secondary)
            KeycapRow(keys: keys)
        }
    }
}
