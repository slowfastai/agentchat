import SwiftUI
#if os(iOS)
import UIKit
#endif

struct ThreadParticipantChip: View {
    let participant: DaemonThreadParticipant
    let color: Color
    let avatarData: Data?
    let customName: String?

    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme {
        Theme(colorScheme: colorScheme)
    }

    private var displayName: String {
        customName ?? participant.displayName
    }

    var body: some View {
        HStack(spacing: 10) {
            avatarView
                .frame(width: 30, height: 30)

            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 4) {
                    Text(displayName)
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(theme.ink)
                        .lineLimit(1)

                    if customName != nil {
                        Image(systemName: "pencil.circle.fill")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
                Text(participant.kindTitle)
                    .font(.caption)
                    .foregroundStyle(theme.mutedInk)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(theme.paper, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .stroke(theme.stroke, lineWidth: 1)
        )
    }

    @ViewBuilder
    private var avatarView: some View {
        if let data = avatarData, let uiImage = UIImage(data: data) {
            Image(uiImage: uiImage)
                .resizable()
                .aspectRatio(contentMode: .fill)
                .clipShape(Circle())
        } else {
            ZStack {
                RoundedRectangle(cornerRadius: 11, style: .continuous)
                    .fill(color.opacity(0.11))
                Text(initials)
                    .font(.caption.weight(.bold))
                    .foregroundStyle(color)
            }
        }
    }

    private var initials: String {
        let parts = displayName.split(separator: " ")
        let value = parts.prefix(2).compactMap { $0.first }.map(String.init).joined()
        return value.isEmpty ? "?" : value.uppercased()
    }
}
