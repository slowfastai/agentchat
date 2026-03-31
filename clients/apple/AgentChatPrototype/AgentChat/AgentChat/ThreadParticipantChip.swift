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
    var onAvatarTap: (() -> Void)? = nil

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
            avatarButton

            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 4) {
                    Text(displayName)
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(.primary)
                        .lineLimit(1)

                    if trimmedCustomName != nil {
                        Image(systemName: "pencil.circle.fill")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }

                Text(isSelectable ? "@\(participant.mentionHandle)" : participant.kindTitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            if isSelectable {
                Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(isSelected ? color : .secondary)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(
            (isSelectable && isSelected ? color.opacity(0.09) : Color(uiColor: .secondarySystemGroupedBackground)),
            in: RoundedRectangle(cornerRadius: 16, style: .continuous)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .stroke(isSelectable && isSelected ? color.opacity(0.28) : Color(uiColor: .separator).opacity(0.18), lineWidth: 1)
        )
        .opacity(isSelectable && !isSelected ? 0.72 : 1)
    }

    private var initials: String {
        let parts = displayName.split(separator: " ")
        let value = parts.prefix(2).compactMap { $0.first }.map(String.init).joined()
        return value.isEmpty ? "?" : value.uppercased()
    }

    @ViewBuilder
    private var avatarButton: some View {
        let avatar = ThreadParticipantAvatarView(
            color: color,
            avatarData: avatarData,
            avatarCacheID: participant.agentID.map { "agent-\($0)" },
            avatarAssetName: participant.defaultAvatarAssetName,
            initials: initials,
            size: 30,
            cornerRadius: 15
        )

        if let onAvatarTap {
            Button(action: onAvatarTap) {
                avatar
                    .overlay(alignment: .bottomTrailing) {
                        ZStack {
                            Circle()
                                .fill(Color(uiColor: .systemBackground))
                                .frame(width: 12, height: 12)

                            Image(systemName: "slider.horizontal.3")
                                .font(.system(size: 6, weight: .bold))
                                .foregroundStyle(color)
                        }
                    }
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Open \(displayName) settings")
        } else {
            avatar
        }
    }
}

struct MentionSuggestionRow: View {
    let participant: DaemonThreadParticipant
    let color: Color
    var avatarData: Data? = nil
    var customName: String? = nil

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
                avatarCacheID: participant.agentID.map { "agent-\($0)" },
                avatarAssetName: participant.defaultAvatarAssetName,
                initials: initials,
                size: 32,
                cornerRadius: 16
            )

            VStack(alignment: .leading, spacing: 2) {
                Text("@\(participant.mentionHandle)")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.primary)

                HStack(spacing: 4) {
                    Text(displayName)
                        .font(.caption)
                        .foregroundStyle(.secondary)
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
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(Color(uiColor: .secondarySystemGroupedBackground))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .stroke(Color(uiColor: .separator).opacity(0.18), lineWidth: 1)
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
    let avatarCacheID: String?
    let avatarAssetName: String?
    let initials: String
    let size: CGFloat
    let cornerRadius: CGFloat

    var body: some View {
        Group {
            if let avatarData,
               let avatarCacheID,
               let image = AgentAvatarImageCache.decodedImage(
                   from: avatarData,
                   cacheID: avatarCacheID
               ) {
                Image(uiImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .clipShape(Circle())
            } else if let avatarData, let image = UIImage(data: avatarData) {
                Image(uiImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .clipShape(Circle())
            } else if let avatarAssetName {
                AgentDefaultAvatarArtwork(
                    assetName: avatarAssetName,
                    size: size,
                    shape: .circle
                )
            } else {
                ZStack {
                    Circle()
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
