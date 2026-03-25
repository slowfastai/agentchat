import SwiftUI

private enum AppTab: Hashable {
    case feed
    case agents
}

struct ContentView: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @StateObject private var store = DaemonChatStore()
    @State private var draft = ""
    @State private var isScannerPresented = false
    @State private var compactPresentedThreadID: String?
    @State private var selectedTab: AppTab = .feed

    var body: some View {
        TabView(selection: $selectedTab) {
            feedRoot
                .tabItem {
                    Label("Feed", systemImage: "bubble.left.and.bubble.right.fill")
                }
                .tag(AppTab.feed)

            agentsRoot
                .tabItem {
                    Label("Agents", systemImage: "person.2.fill")
                }
                .tag(AppTab.agents)
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
        .sheet(isPresented: $isScannerPresented) {
            DaemonQRCodeScannerSheet { payload in
                store.applyScannedConnectionPayload(payload)
            }
        }
        .sheet(item: compactPresentedThreadSheetBinding) { _ in
            NavigationStack {
                detail
                    .toolbar {
                        ToolbarItem(placement: .topBarTrailing) {
                            Button("Done") {
                                compactPresentedThreadID = nil
                            }
                        }
                    }
            }
        }
    }

    @ViewBuilder
    private var feedRoot: some View {
        if horizontalSizeClass == .compact {
            NavigationStack {
                feedList
            }
        } else {
            NavigationSplitView {
                feedList
            } detail: {
                detail
            }
        }
    }

    private var agentsRoot: some View {
        NavigationStack {
            List {
                agentSelectionSection
            }
            .navigationTitle("Agents")
        }
    }

    private var feedList: some View {
        List {
            connectionSection
            threadsSection
        }
        .navigationTitle("Feed")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                feedMenu
            }
        }
    }

    private var connectionSection: some View {
        Section("Connection") {
            HStack(spacing: 10) {
                Circle()
                    .fill(store.connectionStatus.contains("Connected") || store.connectionStatus.contains("Synced") ? Color.green : Color.orange)
                    .frame(width: 10, height: 10)
                Text(store.connectionStatus)
                    .font(.subheadline)
            }
            .padding(.vertical, 4)

            VStack(alignment: .leading, spacing: 6) {
                Text("Daemon URL")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                Text(store.daemonURL)
                    .font(.footnote.monospaced())
                    .textSelection(.enabled)
            }
            .padding(.vertical, 4)

            HStack {
                Button {
                    isScannerPresented = true
                } label: {
                    Label("Scan QR", systemImage: "qrcode.viewfinder")
                }

                Spacer()

                Button {
                    store.reconnectNow()
                } label: {
                    Label("Reconnect", systemImage: "arrow.clockwise")
                }
            }

            Text("Encode the QR as ws://..., wss://..., or agentchat://connect?url=<websocket-url>.")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    private var agentSelectionSection: some View {
        Section("Available Agents") {
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

                Text("Selected agents are used by Feed → menu → Create Thread / Add Agent.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var threadsSection: some View {
        Section("Threads") {
            if store.threads.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("No threads yet")
                        .foregroundStyle(.secondary)
                    Text("Use the top-right menu to create a new thread.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
            } else {
                ForEach(Array(store.threads.enumerated()), id: \.element.threadID) { _, thread in
                    Button {
                        openThread(thread.threadID)
                    } label: {
                        ThreadFeedRow(thread: thread, isActive: store.activeThreadID == thread.threadID)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private var feedMenu: some View {
        Menu {
            Button {
                store.createThreadWithSelectedAgents()
                selectedTab = .feed
            } label: {
                Label("Create Thread", systemImage: "plus.bubble")
            }
            .disabled(store.agents.filter(\.isOnline).isEmpty)

            Button {
                store.addSelectedAgentsToActiveThread()
                selectedTab = .feed
            } label: {
                Label("Add Agent", systemImage: "person.badge.plus")
            }
            .disabled(store.activeThreadID == nil || store.agents.filter(\.isOnline).isEmpty)

            Button {
                isScannerPresented = true
            } label: {
                Label("Scan", systemImage: "qrcode.viewfinder")
            }
        } label: {
            Image(systemName: "plus.circle.fill")
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
            } else if store.activeThreadID != nil {
                UnavailableStateView(
                    title: "Loading Thread",
                    systemImage: "clock.arrow.trianglehead.2.counterclockwise.rotate.90",
                    message: "Waiting for the daemon to attach and replay the thread timeline."
                )
            } else {
                UnavailableStateView(
                    title: "No Active Thread",
                    systemImage: "bubble.left.and.bubble.right",
                    message: "Open a thread from Feed, or create one from Agents."
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
            .onChange(of: store.timeline.count) { _ in
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

    private var compactPresentedThreadSheetBinding: Binding<CompactPresentedThread?> {
        Binding<CompactPresentedThread?>(
            get: {
                compactPresentedThreadID.map { CompactPresentedThread(id: $0) }
            },
            set: { newValue in
                compactPresentedThreadID = newValue?.id
            }
        )
    }

    private func openThread(_ threadID: String) {
        store.attachThread(threadID)
        if horizontalSizeClass == .compact {
            compactPresentedThreadID = threadID
        }
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

private struct CompactPresentedThread: Identifiable {
    let id: String
}

private struct ThreadFeedRow: View {
    let thread: DaemonThreadSummary
    let isActive: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .fill(Color.accentColor.opacity(0.12))
                    .frame(width: 52, height: 52)

                Image(systemName: thread.participantCount > 1 ? "person.2.fill" : "message.fill")
                    .font(.system(size: 20, weight: .semibold))
                    .foregroundStyle(Color.accentColor)
            }

            VStack(alignment: .leading, spacing: 4) {
                HStack(alignment: .firstTextBaseline) {
                    Text(thread.title ?? thread.threadID)
                        .font(.body.weight(.medium))
                        .foregroundStyle(.primary)
                        .lineLimit(1)

                    Spacer(minLength: 8)

                    if isActive {
                        Image(systemName: "bubble.left.and.bubble.right.fill")
                            .foregroundStyle(Color.accentColor)
                    }
                }

                Text("\(thread.participantCount) participants · seq \(thread.lastThreadSeq)")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Text(thread.state.replacingOccurrences(of: "_", with: " ").capitalized)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

private struct UnavailableStateView: View {
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
