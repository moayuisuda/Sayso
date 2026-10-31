import SwiftUI

/// 列表行容器：圆角内边距，悬停显示浅底色，选中显示主题色底色。
struct ListRow<Content: View>: View {
    var isSelected = false
    @ViewBuilder let content: Content

    @State private var isHovering = false

    var body: some View {
        content
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(fill, in: RoundedRectangle(cornerRadius: Theme.Radius.row, style: .continuous))
            .contentShape(Rectangle())
            .onHover { isHovering = $0 }
    }

    private var fill: Color {
        if isSelected { return Theme.selectionFill }
        return isHovering ? Theme.hoverFill : .clear
    }
}
