import SwiftUI

struct ContentView: View {
    @StateObject private var store = DaemonChatStore()
    @State private var draft = ""

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            detail
        }
        .task {
            store.start()
        }
        .alert("Daemon Error", isPresented: Binding(
            get: { store.errorMessage != nil },
            set: { newValue in
                if !newValue {
                    store.errorMessage = nil
                }
            }
        )) {
            Button("OK", role: .cancel) {
                store.errorMessage = nil
            }
        } message: {
            Text(store.errorMessage ?? "Unknown error")
        }
    }

    private var sidebar: some View {
        List {
            connectionSection
            agentSelectionSection
            threadsSection
        }
        .navigationTitle("AgentChat")
    }

    private var connectionSection: some View {
        Section("Connection") {
            HStack {
                Circle()
                    .fill(store.connectionStatus.contains("Connected") || store.connectionStatus.contains("Synced") ? Color.green : Color.orange)
                    .frame(width: 10, height: 10)
                Text(store.connectionStatus)
                    .font(.subheadline)
            }
            .padding(.vertical, 4)
        }
    }

    private var agentSelectionSection: some View {
        Section("Start Group Chat") {
            if store.agents.isEmpty {
                Text("Waiting for daemon agents…")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(store.agents, id: \.agentID) { agent in
                    Button {
                        store.toggleAgentSelection(agent.agentID)
                    } label: {
                        HStack(spacing: 12) {
                            Image(systemName: store.isSelectedAgent(agent.agentID) ? "checkmark.circle.fill" : "circle")
                                .foregroundStyle(store.isSelectedAgent(agent.agentID) ? Color.accentColor : Color.secondary)
                            VStack(alignment: .leading, spacing: 2) {
                                HStack {
                                    Text(agent.name)
                                        .font(.body.weight(.medium))
                                    Spacer()
                                    Text(agent.status.replacingOccurrences(of: "_", with: " ").capitalized)
                                        .font(.caption)
                                        .foregroundStyle(agent.isOnline ? .green : .secondary)
                                }
                                Text(agent.capabilities.joined(separator: " · "))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                    }
                    .buttonStyle(.plain)
                }

                Button {
                    store.createThreadWithSelectedAgents()
                } label: {
                    Label("Create Thread", systemImage: "plus.bubble.fill")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .disabled(store.agents.filter(\.isOnline).isEmpty)
            }
        }
    }

    private var threadsSection: some View {
        Section("Threads") {
            if store.threads.isEmpty {
                Text("No threads yet")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(Array(store.threads.enumerated()), id: \.element.threadID) { _, thread in
                    Button {
                        store.attachThread(thread.threadID)
                    } label: {
                        HStack(alignment: .top) {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(thread.title ?? thread.threadID)
                                    .font(.body.weight(.medium))
                                    .foregroundStyle(.primary)
                                Text("\(thread.participantCount) participants · seq \(thread.lastThreadSeq)")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            if store.activeThreadID == thread.threadID {
                                Image(systemName: "bubble.left.and.bubble.right.fill")
                                    .foregroundStyle(Color.accentColor)
                            }
                        }
                        .padding(.vertical, 4)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private var detail: some View {
        Group {
            if let snapshot = store.activeThreadSnapshot {
                VStack(spacing: 0) {
                    header(snapshot: snapshot)
                    Divider()
                    timeline(snapshot: snapshot)
                    Divider()
                    composer(snapshot: snapshot)
                }
                .navigationTitle(snapshot.title ?? snapshot.threadID)
            } else {
                ContentUnavailableView(
                    "No Active Thread",
                    systemImage: "bubble.left.and.bubble.right",
                    description: Text("Create a thread from the sidebar, then add agents and start chatting through the daemon.")
                )
            }
        }
    }

    private func header(snapshot: DaemonThreadSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(snapshot.title ?? snapshot.threadID)
                .font(.title2.weight(.bold))
            Text("Working dir: \(snapshot.workingDir)")
                .font(.caption)
                .foregroundStyle(.secondary)
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    ForEach(snapshot.participants, id: \.participantID) { participant in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(participant.displayName)
                                .font(.subheadline.weight(.semibold))
                            Text(participant.kind.capitalized)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .background(chipColor(for: participant).opacity(0.12), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                    }
                }
            }
        }
        .padding(20)
    }

    private func timeline(snapshot: DaemonThreadSnapshot) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    ForEach(store.timeline, id: \.id) { entry in
                        TimelineBubble(entry: entry)
                            .id(entry.id)
                    }
                }
                .padding(20)
            }
            .background(Color(uiColor: .systemGroupedBackground))
            .onChange(of: store.timeline.count) { _, _ in
                if let lastID = store.timeline.last?.id {
                    withAnimation(.easeOut(duration: 0.2)) {
                        proxy.scrollTo(lastID, anchor: .bottom)
                    }
                }
            }
        }
    }

    private func composer(snapshot: DaemonThreadSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            if !snapshot.participants.filter(\.isAgent).isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(snapshot.participants.filter(\.isAgent), id: \.participantID) { participant in
                            Button {
                                store.toggleParticipantSelection(participant.participantID)
                            } label: {
                                HStack(spacing: 6) {
                                    Image(systemName: store.isSelectedParticipant(participant.participantID) ? "checkmark.circle.fill" : "circle")
                                    Text(participant.displayName)
                                }
                                .font(.subheadline)
                                .padding(.horizontal, 10)
                                .padding(.vertical, 8)
                                .background(chipColor(for: participant).opacity(0.15), in: Capsule())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }

            HStack(alignment: .bottom, spacing: 12) {
                TextField("Send a message to the thread", text: $draft, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(1...4)
                Button {
                    let message = draft
                    draft = ""
                    store.sendCurrentMessage(message)
                } label: {
                    Image(systemName: "paperplane.fill")
                        .font(.system(size: 18, weight: .semibold))
                }
                .buttonStyle(.borderedProminent)
                .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(20)
        .background(Color(uiColor: .secondarySystemGroupedBackground))
    }

    private func chipColor(for participant: DaemonThreadParticipant) -> Color {
        switch participant.agentID?.lowercased() {
        case "pi": return .purple
        case "beta": return .green
        case "alpha", "claude": return .blue
        case "codex": return .green
        case "opencode": return .orange
        default: return participant.isAgent ? .indigo : .gray
        }
    }
}

private struct TimelineBubble: View {
    let entry: DaemonTimelineEntry

    var body: some View {
        HStack(alignment: .top) {
            if entry.kind == .user {
                Spacer(minLength: 60)
            }

            VStack(alignment: .leading, spacing: 6) {
                Text(entry.title)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                Text(entry.body.isEmpty ? "…" : entry.body)
                    .font(.body)
                    .foregroundStyle(entry.kind == .user ? .white : .primary)
                Text("seq \(entry.threadSeq)")
                    .font(.caption2)
                    .foregroundStyle(entry.kind == .user ? .white.opacity(0.85) : .secondary)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            .background(backgroundColor, in: RoundedRectangle(cornerRadius: 16, style: .continuous))

            if entry.kind != .user {
                Spacer(minLength: 60)
            }
        }
    }

    private var backgroundColor: Color {
        switch entry.kind {
        case .user:
            return .blue
        case .thinking:
            return Color.primary.opacity(0.06)
        case .tool:
            return .orange.opacity(0.12)
        case .plan:
            return .purple.opacity(0.12)
        case .turnEnd:
            return .green.opacity(0.14)
        case .system:
            return .gray.opacity(0.12)
        case .agentMessage:
            return tint.opacity(0.14)
        }
    }

    private var tint: Color {
        switch entry.tintName {
        case "purple": return .purple
        case "green": return .green
        case "orange": return .orange
        case "blue": return .blue
        case "red": return .red
        default: return .indigo
        }
    }
}
