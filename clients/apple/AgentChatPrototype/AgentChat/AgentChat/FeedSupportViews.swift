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
    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme {
        Theme(colorScheme: colorScheme)
    }

    var body: some View {
        ZStack {
            LinearGradient(
                colors: [theme.canvasTop, theme.canvasBottom],
                startPoint: .top,
                endPoint: .bottom
            )

            Circle()
                .fill(theme.accentWarm.opacity(0.10))
                .frame(width: 300, height: 300)
                .blur(radius: 90)
                .offset(x: -160, y: -280)

            Circle()
                .fill(Color.white.opacity(colorScheme == .dark ? 0.05 : 0.38))
                .frame(width: 220, height: 220)
                .blur(radius: 40)
                .offset(x: 170, y: -180)
        }
        .ignoresSafeArea()
    }
}

struct HeaderInfoPill: View {
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
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(theme.chip, in: Capsule())
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

    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme {
        Theme(colorScheme: colorScheme)
    }

    var body: some View {
        Label(text, systemImage: icon)
            .font(.caption.weight(.medium))
            .foregroundStyle(theme.mutedInk)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(theme.paper.opacity(0.95), in: Capsule())
            .overlay(
                Capsule()
                    .stroke(theme.stroke, lineWidth: 1)
            )
    }
}

struct EmptyThreadState: View {
    let snapshot: DaemonThreadSnapshot

    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme {
        Theme(colorScheme: colorScheme)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Begin a calm, focused thread")
                .font(.headline)
                .foregroundStyle(theme.ink)

            Text("Ask for a summary, a code change, or route work to one or more agents. Responses will unfold here with more room to read.")
                .font(.subheadline)
                .foregroundStyle(theme.mutedInk)

            Text(snapshot.participants.map(\.displayName).joined(separator: " · "))
                .font(.caption)
                .foregroundStyle(theme.subtleInk)
        }
        .padding(22)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .fill(theme.panel)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .stroke(theme.stroke, lineWidth: 1)
        )
    }
}

struct TargetSelectionChip: View {
    let participant: DaemonThreadParticipant
    let color: Color
    let isSelected: Bool

    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme {
        Theme(colorScheme: colorScheme)
    }

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(isSelected ? theme.accentWarm : theme.subtleInk)

            Text(participant.displayName)
                .font(.subheadline.weight(.medium))
                .foregroundStyle(theme.ink)
                .lineLimit(1)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 9)
        .background(
            RoundedRectangle(cornerRadius: 15, style: .continuous)
                .fill(isSelected ? theme.toolPanel : theme.paper)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 15, style: .continuous)
                .stroke(isSelected ? theme.accentWarm.opacity(0.25) : theme.stroke, lineWidth: 1)
        )
    }
}

struct ThreadFeedRow: View {
    let thread: DaemonThreadSummary
    let isActive: Bool
    let isPinned: Bool
    let avatarItems: [ThreadFeedAvatarItem]

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            ThreadFeedAvatarStack(
                avatarItems: avatarItems,
                participantCount: thread.participantCount
            )

            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(thread.title ?? thread.threadID)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)

                    if isPinned {
                        Image(systemName: "pin.fill")
                            .font(.caption)
                            .foregroundStyle(.orange)
                    }
                }

                HStack(spacing: 8) {
                    SmallInfoPill(icon: "person.2.fill", text: "\(thread.participantCount)")
                }

                Text(thread.workingDir)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)

            VStack(alignment: .trailing, spacing: 8) {
                if isActive {
                    Image(systemName: "bubble.left.and.bubble.right.fill")
                        .foregroundStyle(Color.accentColor)
                }

                Text(thread.state.replacingOccurrences(of: "_", with: " ").capitalized)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.secondary)
            }
        }
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .fill(Color(uiColor: .secondarySystemBackground).opacity(0.96))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .stroke(isActive ? Color.accentColor.opacity(0.35) : Color.black.opacity(0.06), lineWidth: 1)
        )
        .shadow(color: Color.black.opacity(isActive ? 0.08 : 0.03), radius: 16, y: 8)
        .contentShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
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
        .frame(width: 56, height: 56)
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
