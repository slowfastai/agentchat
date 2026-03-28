import SwiftUI
#if os(iOS)
import UIKit
#endif

struct ThreadParticipantChip: View {
    let participant: DaemonThreadParticipant
    let color: Color
    var avatarData: Data? = nil
    var customName: String? = nil
    var isSelected: Bool = true
    var isSelectable: Bool = false

    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme {
        Theme(colorScheme: colorScheme)
    }

    private var trimmedCustomName: String? {
        guard let customName else { return nil }
        let trimmed = customName.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private var displayName: String {
        trimmedCustomName ?? participant.displayName
    }

    var body: some View {
        HStack(spacing: 10) {
            ThreadParticipantAvatarView(
                color: color,
                avatarData: avatarData,
                initials: initials,
                size: 30,
                cornerRadius: 11
            )

            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 4) {
                    Text(displayName)
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(theme.ink)
                        .lineLimit(1)

                    if trimmedCustomName != nil {
                        Image(systemName: "pencil.circle.fill")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }

                Text(isSelectable ? "@\(participant.mentionHandle)" : participant.kindTitle)
                    .font(.caption)
                    .foregroundStyle(theme.mutedInk)
                    .lineLimit(1)
            }

            if isSelectable {
                Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(isSelected ? color : theme.subtleInk)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(
            (isSelectable && isSelected ? color.opacity(0.09) : theme.paper),
            in: RoundedRectangle(cornerRadius: 16, style: .continuous)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .stroke(isSelectable && isSelected ? color.opacity(0.28) : theme.stroke, lineWidth: 1)
        )
        .opacity(isSelectable && !isSelected ? 0.72 : 1)
    }

    private var initials: String {
        let parts = displayName.split(separator: " ")
        let value = parts.prefix(2).compactMap { $0.first }.map(String.init).joined()
        return value.isEmpty ? "?" : value.uppercased()
    }
}

struct MentionSuggestionRow: View {
    let participant: DaemonThreadParticipant
    let color: Color
    var avatarData: Data? = nil
    var customName: String? = nil

    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme {
        Theme(colorScheme: colorScheme)
    }

    private var trimmedCustomName: String? {
        guard let customName else { return nil }
        let trimmed = customName.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private var displayName: String {
        trimmedCustomName ?? participant.displayName
    }

    var body: some View {
        HStack(spacing: 12) {
            ThreadParticipantAvatarView(
                color: color,
                avatarData: avatarData,
                initials: initials,
                size: 32,
                cornerRadius: 10
            )

            VStack(alignment: .leading, spacing: 2) {
                Text("@\(participant.mentionHandle)")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(theme.ink)

                HStack(spacing: 4) {
                    Text(displayName)
                        .font(.caption)
                        .foregroundStyle(theme.mutedInk)
                        .lineLimit(1)

                    if trimmedCustomName != nil {
                        Image(systemName: "pencil.circle.fill")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
            }

            Spacer(minLength: 0)

            Text(participant.kindTitle)
                .font(.caption2.weight(.medium))
                .foregroundStyle(theme.subtleInk)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(theme.panel)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .stroke(theme.stroke, lineWidth: 1)
        )
    }

    private var initials: String {
        let parts = displayName.split(separator: " ")
        let value = parts.prefix(2).compactMap { $0.first }.map(String.init).joined()
        return value.isEmpty ? "?" : value.uppercased()
    }
}

private struct ThreadParticipantAvatarView: View {
    let color: Color
    let avatarData: Data?
    let initials: String
    let size: CGFloat
    let cornerRadius: CGFloat

    var body: some View {
        Group {
            if let avatarData, let image = UIImage(data: avatarData) {
                Image(uiImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .clipShape(Circle())
            } else {
                ZStack {
                    RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                        .fill(color.opacity(0.11))
                    Text(initials)
                        .font(.caption.weight(.bold))
                        .foregroundStyle(color)
                }
            }
        }
        .frame(width: size, height: size)
    }
}
