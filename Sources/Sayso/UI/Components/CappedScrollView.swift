import SwiftUI

/// 高度随内容增长、超过上限后改为滚动的区域。
///
/// 悬浮候选框的尺寸由内容决定，普通 ScrollView 在这种布局下没有确定的高度，因此先测量内容高度，
/// 再取「内容高度」与「上限」中的较小值作为滚动区域高度。
struct CappedScrollView<Content: View>: View {
    let maxHeight: CGFloat
    /// 该值变化时滚动到底部，用于跟随实时增长的文本；为 nil 时不自动滚动。
    var followValue: AnyHashable?
    @ViewBuilder let content: Content

    @State private var contentHeight: CGFloat = 0
    private let bottomID = "bottom"

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 0) {
                    content
                    Color.clear.frame(height: 0).id(bottomID)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { contentHeight = $0 }
            }
            .scrollIndicators(contentHeight > maxHeight ? .automatic : .never)
            .frame(height: min(contentHeight, maxHeight))
            .onChange(of: followValue) { _, _ in
                guard followValue != nil else { return }
                proxy.scrollTo(bottomID, anchor: .bottom)
            }
            .onChange(of: contentHeight) { _, _ in
                guard followValue != nil else { return }
                proxy.scrollTo(bottomID, anchor: .bottom)
            }
        }
    }
}
