import SwiftUI
#if os(iOS)
import UIKit
#endif

struct ThreadFeedAvatarItem: Identifiable, Hashable {
    let id: String
    let tintColor: Color
    let avatarData: Data?
    let avatarAssetName: String?
    let initials: String
}

struct ChatScreenBackground: View {
    var body: some View {
        Color(uiColor: .systemGroupedBackground)
            .ignoresSafeArea()
    }
}

struct HeaderInfoPill: View {
    let icon: String
    let text: String

    var body: some View {
        Label(text, systemImage: icon)
            .font(.caption.weight(.medium))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(Color(uiColor: .secondarySystemBackground), in: Capsule())
            .overlay(
                Capsule()
                    .stroke(Color(uiColor: .separator).opacity(0.18), lineWidth: 1)
            )
    }
}

struct TimelineHeroStrip: View {
    let eventCount: Int
    let connectionStatus: String
    let participantCount: Int

    var body: some View {
        HStack(spacing: 10) {
            SmallInfoPill(icon: "bubble.left.and.bubble.right", text: "\(eventCount) messages")
            SmallInfoPill(icon: "person.2", text: "\(participantCount) agents")
            Spacer(minLength: 0)
            SmallInfoPill(icon: "sparkles", text: connectionStatus)
        }
    }
}

struct SmallInfoPill: View {
    let icon: String
    let text: String

    var body: some View {
        Label(text, systemImage: icon)
            .font(.caption.weight(.medium))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(Color(uiColor: .secondarySystemBackground), in: Capsule())
            .overlay(
                Capsule()
                    .stroke(Color(uiColor: .separator).opacity(0.18), lineWidth: 1)
            )
    }
}

struct EmptyThreadState: View {
    let snapshot: DaemonThreadSnapshot

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Begin a calm, focused thread")
                .font(.headline)
                .foregroundStyle(.primary)

            Text("Ask for a summary, a code change, or route work to one or more agents. Responses will unfold here with more room to read.")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            Text(snapshot.participants.map(\.displayName).joined(separator: " · "))
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(22)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .fill(Color(uiColor: .secondarySystemGroupedBackground))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .stroke(Color(uiColor: .separator).opacity(0.18), lineWidth: 1)
        )
    }
}

struct TargetSelectionChip: View {
    let participant: DaemonThreadParticipant
    let color: Color
    let isSelected: Bool

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(isSelected ? color : .secondary)

            Text(participant.displayName)
                .font(.subheadline.weight(.medium))
                .foregroundStyle(.primary)
                .lineLimit(1)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 9)
        .background(
            RoundedRectangle(cornerRadius: 15, style: .continuous)
                .fill(isSelected ? color.opacity(0.10) : Color(uiColor: .secondarySystemGroupedBackground))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 15, style: .continuous)
                .stroke(isSelected ? color.opacity(0.25) : Color(uiColor: .separator).opacity(0.18), lineWidth: 1)
        )
    }
}

struct ThreadFeedRow: View {
    let thread: DaemonThreadSummary
    let isActive: Bool
    let isPinned: Bool
    let avatarItems: [ThreadFeedAvatarItem]

    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme {
        Theme(colorScheme: colorScheme)
    }

    private var displayTitle: String {
        let trimmedTitle = thread.title?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return trimmedTitle.isEmpty ? thread.threadID : trimmedTitle
    }

    private var normalizedStateTitle: String {
        thread.state
            .replacingOccurrences(of: "_", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .capitalized
    }

    private var shortWorkingDirectory: String? {
        let trimmed = thread.workingDir.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, trimmed != ".", trimmed != "./" else {
            return nil
        }

        let normalized = trimmed.hasSuffix("/") ? String(trimmed.dropLast()) : trimmed
        let lastComponent = NSString(string: normalized).lastPathComponent
        return lastComponent.isEmpty ? normalized : lastComponent
    }

    private var stateTint: Color {
        let normalized = thread.state.lowercased()

        if normalized.contains("run") || normalized.contains("stream") || normalized.contains("busy") {
            return theme.accent
        }
        if normalized.contains("error") || normalized.contains("fail") {
            return .red
        }
        if normalized.contains("wait") || normalized.contains("input") || normalized.contains("pending") {
            return .orange
        }
        return theme.subtleInk
    }

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            ThreadFeedAvatarStack(
                avatarItems: avatarItems,
                participantCount: thread.participantCount
            )

            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text(displayTitle)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(theme.ink)
                        .lineLimit(1)

                    Spacer(minLength: 0)

                    ThreadStateBadge(
                        title: normalizedStateTitle,
                        tint: stateTint,
                        isActive: isActive
                    )
                }

                HStack(spacing: 8) {
                    ThreadMetaChip(icon: "person.2.fill", text: "\(thread.participantCount)")

                    if let shortWorkingDirectory {
                        ThreadMetaChip(icon: "folder.fill", text: shortWorkingDirectory)
                    }

                    if isPinned {
                        ThreadMetaChip(icon: "pin.fill", text: "Pinned")
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 13)
        .background(
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .fill(theme.panel.opacity(0.98))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .stroke(isActive ? stateTint.opacity(0.30) : theme.stroke, lineWidth: 1)
        )
        .overlay(alignment: .leading) {
            RoundedRectangle(cornerRadius: 2, style: .continuous)
                .fill(isActive ? stateTint : .clear)
                .frame(width: 3, height: 38)
                .padding(.leading, 8)
        }
        .shadow(color: Color.black.opacity(isActive ? 0.05 : 0.02), radius: 12, y: 5)
        .contentShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
    }
}

private struct ThreadFeedAvatarStack: View {
    let avatarItems: [ThreadFeedAvatarItem]
    let participantCount: Int

    var body: some View {
        let visibleItems = Array(avatarItems.prefix(2))

        ZStack {
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .fill(
                    LinearGradient(
                        colors: [Color.accentColor.opacity(0.22), Color.accentColor.opacity(0.10)],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )

            if visibleItems.isEmpty {
                Image(systemName: participantCount > 1 ? "person.2.fill" : "message.fill")
                    .font(.system(size: 20, weight: .semibold))
                    .foregroundStyle(Color.accentColor)
            } else if visibleItems.count == 1 {
                ThreadFeedAvatarBubble(item: visibleItems[0], size: 40)
            } else {
                ZStack {
                    ThreadFeedAvatarBubble(item: visibleItems[0], size: 32)
                        .offset(x: -8, y: -4)

                    ThreadFeedAvatarBubble(item: visibleItems[1], size: 32)
                        .offset(x: 8, y: 4)
                }
            }
        }
        .frame(width: 48, height: 48)
    }
}

private struct ThreadFeedAvatarBubble: View {
    let item: ThreadFeedAvatarItem
    let size: CGFloat

    var body: some View {
        Group {
            if let avatarData = item.avatarData,
               let image = UIImage(data: avatarData) {
                Image(uiImage: image)
                    .resizable()
                    .scaledToFill()
            } else if let assetName = item.avatarAssetName {
                Image(assetName)
                    .resizable()
                    .scaledToFill()
            } else {
                ZStack {
                    Circle()
                        .fill(item.tintColor.opacity(0.14))

                    Text(item.initials)
                        .font(.system(size: size * 0.32, weight: .bold))
                        .foregroundStyle(item.tintColor)
                }
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .overlay {
            Circle()
                .stroke(Color.white.opacity(0.92), lineWidth: 2)
        }
        .shadow(color: Color.black.opacity(0.10), radius: 5, y: 2)
    }
}

private struct ThreadMetaChip: View {
    let icon: String
    let text: String

    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme {
        Theme(colorScheme: colorScheme)
    }

    var body: some View {
        Label(text, systemImage: icon)
            .font(.caption.weight(.medium))
            .foregroundStyle(theme.mutedInk)
            .lineLimit(1)
            .padding(.horizontal, 9)
            .padding(.vertical, 6)
            .background(theme.paper, in: Capsule())
            .overlay {
                Capsule()
                    .stroke(theme.stroke, lineWidth: 1)
            }
    }
}

private struct ThreadStateBadge: View {
    let title: String
    let tint: Color
    let isActive: Bool

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(tint)
                .frame(width: 7, height: 7)

            Text(isActive ? "Live" : title)
                .font(.caption.weight(.semibold))
                .lineLimit(1)
        }
        .foregroundStyle(tint)
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .background(tint.opacity(isActive ? 0.16 : 0.10), in: Capsule())
    }
}

struct UnavailableStateView: View {
    let title: String
    let systemImage: String
    let message: String

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: systemImage)
                .font(.system(size: 42, weight: .semibold))
                .foregroundStyle(.secondary)
            Text(title)
                .font(.title3.weight(.semibold))
            Text(message)
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 24)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(24)
    }
}
