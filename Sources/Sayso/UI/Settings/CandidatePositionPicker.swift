import SwiftUI
import SaysoCore

struct CandidatePositionPicker: View {
    let positions: [CandidateCornerOption]
    @Binding var selection: CandidateCorner

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            RoundedRectangle(cornerRadius: 8)
                .fill(Theme.cardFill)
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.secondary))
                .frame(width: 176, height: 100)
                .overlay {
                    ZStack {
                        ForEach(positions, id: \.value) { position in
                            positionButton(position)
                                .padding(3)
                                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: alignment(for: position))
                        }
                    }
                }

            if let current = positions.first(where: { $0.value == selection }) {
                Text(current.title)
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
            }
        }
    }

    private func alignment(for position: CandidateCornerOption) -> Alignment {
        Alignment(horizontal: position.horizontal < 0 ? .leading : .trailing,
                  vertical: position.vertical < 0 ? .top : .bottom)
    }

    private func positionButton(_ position: CandidateCornerOption) -> some View {
        let selected = selection == position.value
        return Button {
            selection = position.value
        } label: {
            ZStack(alignment: alignment(for: position)) {
                Path { path in
                    let x: CGFloat = position.horizontal < 0 ? 1 : 31
                    let y: CGFloat = position.vertical < 0 ? 1 : 21
                    path.move(to: CGPoint(x: x, y: y - CGFloat(position.vertical) * 11))
                    path.addLine(to: CGPoint(x: x, y: y))
                    path.addLine(to: CGPoint(x: x - CGFloat(position.horizontal) * 11, y: y))
                }
                .stroke(Color.secondary, lineWidth: 1)

                if selected {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(Theme.accentForeground)
                        .frame(width: 24, height: 10)
                        .padding(4)
                }
            }
            .frame(width: 32, height: 22)
            .frame(width: 44, height: 34)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(position.title)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .help(position.title)
    }
}
