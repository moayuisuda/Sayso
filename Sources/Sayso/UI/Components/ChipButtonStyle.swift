import SwiftUI

/// 紧凑的浅底按钮：悬停与按下时加深，`destructive` 角色显示为红色。用于块操作与次要按钮。
struct ChipButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        ChipButton(configuration: configuration)
    }
}

extension ButtonStyle where Self == ChipButtonStyle {
    static var chip: ChipButtonStyle { ChipButtonStyle() }
}

private struct ChipButton: View {
    let configuration: ButtonStyleConfiguration

    @Environment(\.isEnabled) private var isEnabled
    @State private var isHovering = false

    var body: some View {
        configuration.label
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(configuration.role == .destructive ? Color.red : Color.primary)
            .padding(.horizontal, 8)
            .frame(height: 22)
            .background(Color.primary.opacity(fillOpacity), in: RoundedRectangle(cornerRadius: Theme.Radius.control, style: .continuous))
            .opacity(isEnabled ? 1 : 0.45)
            .contentShape(Rectangle())
            .onHover { isHovering = $0 }
    }

    private var fillOpacity: Double {
        if configuration.isPressed { return 0.16 }
        return isHovering && isEnabled ? 0.11 : 0.06
    }
}
