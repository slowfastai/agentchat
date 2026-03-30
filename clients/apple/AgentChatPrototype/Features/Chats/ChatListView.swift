import SwiftUI

extension Color {
    static var appCanvasBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .systemGroupedBackground)
        #else
        Color(nsColor: .windowBackgroundColor)
        #endif
    }
}

struct ChatListView: View {
    @EnvironmentObject private var store: DemoStore
    @Binding var selectedIssueID: UUID?
    @State private var searchText = ""

    private var filteredThreads: [ChatThreadSummary] {
        guard !searchText.isEmpty else {
            return store.chatThreads
        }

        return store.chatThreads.filter { thread in
            let searchable = [
                thread.title,
                thread.preview,
                thread.participants.joined(separator: " ")
            ]
            .joined(separator: " ")
            .lowercased()

            return searchable.contains(searchText.lowercased())
        }
    }

    private var pinnedThreads: [ChatThreadSummary] {
        filteredThreads.filter(\.isPinned)
    }

    private var recentThreads: [ChatThreadSummary] {
        filteredThreads.filter { !$0.isPinned }
    }

    var body: some View {
        List {
            if !pinnedThreads.isEmpty {
                Section("Pinned") {
                    ForEach(pinnedThreads) { thread in
                        chatRow(for: thread)
                    }
                }
            }

            Section(pinnedThreads.isEmpty ? "Recent Chats" : "All Chats") {
                ForEach(recentThreads) { thread in
                    chatRow(for: thread)
                }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(Color.appCanvasBackground)
        .searchable(text: $searchText, prompt: "Search chats")
        .navigationTitle("Chats")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                } label: {
                    Image(systemName: "plus")
                }
            }
        }
        #if os(iOS)
        .navigationBarTitleDisplayMode(.large)
        #endif
    }

    @ViewBuilder
    private func chatRow(for thread: ChatThreadSummary) -> some View {
        NavigationLink {
            IssueWorkspaceView(issueID: thread.issueID)
        } label: {
            ChatThreadRow(thread: thread)
        }
        .simultaneousGesture(TapGesture().onEnded {
            selectedIssueID = thread.issueID
        })
    }
}

private struct ChatThreadRow: View {
    let thread: ChatThreadSummary

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            ThreadAvatarView(thread: thread)

            VStack(alignment: .leading, spacing: 4) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(displayTitle)
                        .font(.body.weight(.medium))
                        .foregroundStyle(.primary)
                        .lineLimit(1)

                    Spacer(minLength: 8)

                    Text(timestampText)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                HStack(alignment: .center, spacing: 8) {
                    Text(thread.preview)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)

                    Spacer(minLength: 8)

                    if thread.unreadCount > 0 {
                        UnreadBadge(count: thread.unreadCount)
                    } else if thread.state == .running {
                        Circle()
                            .fill(AppColors.onlineStatus)
                            .frame(width: 9, height: 9)
                    }
                }

                if !thread.participants.isEmpty {
                    Text(thread.participants.joined(separator: " · "))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }

    private var displayTitle: String {
        if thread.isPinned {
            return "📌 #\(thread.issueNumber) \(thread.title)"
        }
        return "#\(thread.issueNumber) \(thread.title)"
    }

    private var timestampText: String {
        AppFormatters.relativeString(from: thread.updatedAt)
    }
}

private struct ThreadAvatarView: View {
    let thread: ChatThreadSummary

    var body: some View {
        let family = DaemonAgentFamily(agentID: nil, kind: nil, name: thread.participants.first)

        ZStack {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(thread.accent.color.opacity(0.15))
                .frame(width: 54, height: 54)

            if thread.participants.count <= 1 {
                if let avatarAssetName = family.defaultAvatarAssetName {
                    AgentDefaultAvatarArtwork(
                        assetName: avatarAssetName,
                        size: 38,
                        shape: .circle
                    )
                } else {
                    Image(systemName: iconName)
                        .font(.system(size: 22, weight: .semibold))
                        .foregroundStyle(thread.accent.color)
                }
            } else {
                ZStack {
                    Circle()
                        .fill(thread.accent.color.opacity(0.95))
                        .frame(width: 24, height: 24)
                        .offset(x: -8, y: -6)

                    Circle()
                        .fill(AppColors.onlineStatus.opacity(0.95))
                        .frame(width: 24, height: 24)
                        .offset(x: 10, y: 8)

                    Image(systemName: "person.2.fill")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(.white)
                }
            }
        }
    }

    private var iconName: String {
        let participant = thread.participants.first ?? ""
        switch participant {
        case "Claude": return "brain.head.profile"
        case "Codex": return "curlybraces.square.fill"
        case "Pi": return "sparkles"
        default: return "message.fill"
        }
    }
}

private struct UnreadBadge: View {
    let count: Int

    var body: some View {
        Text(count > 99 ? "99+" : "\(count)")
            .font(.caption2.weight(.bold))
            .foregroundStyle(.white)
            .padding(.horizontal, 7)
            .padding(.vertical, 4)
            .background(AppColors.unreadBadge, in: Capsule())
    }
}

enum AppColors {
    static var onlineStatus: Color {
        Color(red: 0.3, green: 0.85, blue: 0.5)
    }

    static var unreadBadge: Color {
        Color(red: 1.0, green: 0.35, blue: 0.35)
    }
}
