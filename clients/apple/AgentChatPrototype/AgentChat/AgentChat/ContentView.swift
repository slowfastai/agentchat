import SwiftUI
#if os(iOS)
import UIKit
#endif

private enum AppTab: Hashable {
    case feed
    case agents
    case settings
}

struct ContentView: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @StateObject private var store = DaemonChatStore()
    @State private var draft = ""
    @State private var daemonURLDraft = ""
    @State private var isScannerPresented = false
    @State private var compactPresentedThreadID: String?
    @State private var pendingCloseThread: DaemonThreadSummary?
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

            settingsRoot
                .tabItem {
                    Label("Settings", systemImage: "gearshape.fill")
                }
                .tag(AppTab.settings)
        }
        .tint(.indigo)
        .task {
            daemonURLDraft = store.daemonURL
            store.start()
        }
        .onChange(of: store.daemonURL) { newValue in
            daemonURLDraft = newValue
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
        .confirmationDialog(
            "Close Thread?",
            isPresented: Binding(
                get: { pendingCloseThread != nil },
                set: { newValue in
                    if !newValue {
                        pendingCloseThread = nil
                    }
                }
            ),
            titleVisibility: .visible
        ) {
            if let thread = pendingCloseThread {
                Button("Close in Daemon", role: .destructive) {
                    store.closeThread(thread.threadID)
                    pendingCloseThread = nil
                }
            }

            Button("Cancel", role: .cancel) {
                pendingCloseThread = nil
            }
        } message: {
            if let thread = pendingCloseThread {
                Text("\"\(thread.title ?? thread.threadID)\" will be closed in the daemon and removed for every client, not just hidden on this device.")
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
            .listStyle(.insetGrouped)
            .navigationTitle("Agents")
        }
    }

    private var settingsRoot: some View {
        NavigationStack {
            List {
                settingsConnectionSection
            }
            .listStyle(.insetGrouped)
            .navigationTitle("Settings")
        }
    }

    private var feedList: some View {
        List {
            threadsSection
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(ChatScreenBackground())
        .navigationTitle("Feed")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                feedMenu
            }
        }
    }

    private var agentSelectionSection: some View {
        Section("Available Agents") {
            if store.agents.isEmpty {
                Text("Waiting for daemon agents…")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(store.agents, id: \.agentID) { agent in
                    HStack(alignment: .top, spacing: 12) {
                        ZStack {
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .fill(color(named: agent.tintName).opacity(0.14))

                            Image(systemName: agent.symbolName)
                                .font(.system(size: 18, weight: .semibold))
                                .foregroundStyle(color(named: agent.tintName))
                        }
                        .frame(width: 40, height: 40)

                        Button {
                            store.toggleAgentSelection(agent.agentID)
                        } label: {
                            HStack(spacing: 12) {
                                Image(systemName: store.isSelectedAgent(agent.agentID) ? "checkmark.circle.fill" : "circle")
                                    .foregroundStyle(store.isSelectedAgent(agent.agentID) ? Color.accentColor : Color.secondary)

                                VStack(alignment: .leading, spacing: 2) {
                                    Text(agent.displayName)
                                        .font(.body.weight(.medium))

                                    Text(agentSubtitle(for: agent))
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .buttonStyle(.plain)

                        VStack(alignment: .trailing, spacing: 6) {
                            Text(agent.status.replacingOccurrences(of: "_", with: " ").capitalized)
                                .font(.caption)
                                .foregroundStyle(agentStatusColor(for: agent))

                            if agent.isOffline {
                                Button {
                                    store.reconnectNow()
                                } label: {
                                    Text("Reconnect")
                                        .font(.caption2.weight(.semibold))
                                }
                                .buttonStyle(.bordered)
                                .controlSize(.small)
                                .disabled(!store.hasConfiguredDaemonURL)
                            }
                        }
                    }
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
                .listRowBackground(Color.clear)
            } else {
                ForEach(Array(store.threads.enumerated()), id: \.element.threadID) { _, thread in
                    Button {
                        openThread(thread.threadID)
                    } label: {
                        ThreadFeedRow(
                            thread: thread,
                            isActive: store.activeThreadID == thread.threadID,
                            isPinned: store.isPinnedThread(thread.threadID)
                        )
                    }
                    .buttonStyle(.plain)
                    .listRowInsets(EdgeInsets(top: 6, leading: 0, bottom: 6, trailing: 0))
                    .listRowSeparator(.hidden)
                    .listRowBackground(Color.clear)
                    .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                        Button {
                            store.togglePinnedThread(thread.threadID)
                        } label: {
                            Label(store.isPinnedThread(thread.threadID) ? "Unpin" : "Pin", systemImage: store.isPinnedThread(thread.threadID) ? "pin.slash" : "pin")
                        }
                        .tint(.orange)

                        Button {
                            store.hideThread(thread.threadID)
                        } label: {
                            Label("Hide", systemImage: "eye.slash")
                        }
                        .tint(.gray)

                        Button(role: .destructive) {
                            pendingCloseThread = thread
                        } label: {
                            Label("Close", systemImage: "xmark.circle")
                        }
                    }
                }
            }
        }
    }

    private var settingsConnectionSection: some View {
        Section("Connection") {
            HStack(spacing: 10) {
                Circle()
                    .fill(connectionStatusColor)
                    .frame(width: 10, height: 10)
                Text(store.connectionStatus)
                    .font(.subheadline)
            }
            .padding(.vertical, 4)

            VStack(alignment: .leading, spacing: 8) {
                Text("Daemon URL")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)

                TextField("ws://192.168.1.10:9390", text: $daemonURLDraft)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled(true)
                    .keyboardType(.URL)
                    .font(.footnote.monospaced())
                    .textFieldStyle(.roundedBorder)
                    .onSubmit {
                        submitDaemonURL()
                    }

                if !store.daemonURL.isEmpty {
                    Text("Saved: \(store.daemonURL)")
                        .font(.footnote.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
            .padding(.vertical, 4)

            Button {
                submitDaemonURL()
            } label: {
                Label("Save & Connect", systemImage: "link.badge.plus")
            }
            .disabled(daemonURLDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

            Button {
                isScannerPresented = true
            } label: {
                Label("Scan QR", systemImage: "qrcode.viewfinder")
            }

            Button {
                store.reconnectNow()
            } label: {
                Label("Reconnect", systemImage: "arrow.clockwise")
            }
            .disabled(!store.hasConfiguredDaemonURL)

            Text("The app will not auto-connect on first launch. Connect only by scanning a QR code, entering a URL, or tapping Reconnect.")
                .font(.footnote)
                .foregroundStyle(.secondary)

            Text("Encode the QR as ws://..., wss://..., or agentchat://connect?url=<websocket-url>&agents=<comma-separated-agent-ids>.")
                .font(.footnote)
                .foregroundStyle(.secondary)
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
                timeline(snapshot: snapshot)
                    .background(ChatScreenBackground().ignoresSafeArea())
                    .safeAreaInset(edge: .bottom, spacing: 0) {
                        composer(snapshot: snapshot)
                            .padding(.horizontal, 16)
                            .padding(.bottom, 10)
                            .background(.clear)
                    }
                    .navigationTitle(snapshot.title ?? snapshot.threadID)
                    .navigationBarTitleDisplayMode(.inline)
            } else if store.activeThreadID != nil {
                UnavailableStateView(
                    title: "Loading Thread",
                    systemImage: "clock.arrow.trianglehead.2.counterclockwise.rotate.90",
                    message: "Waiting for the daemon to attach and replay the thread timeline."
                )
                .background(ChatScreenBackground().ignoresSafeArea())
            } else {
                UnavailableStateView(
                    title: "No Active Thread",
                    systemImage: "bubble.left.and.bubble.right",
                    message: "Open a thread from Feed, or create one from Agents."
                )
                .background(ChatScreenBackground().ignoresSafeArea())
            }
        }
        .toolbar {
            ToolbarItemGroup(placement: .keyboard) {
                Spacer()
                Button("Done") {
                    dismissKeyboard()
                }
            }
        }
    }

    private var activeClosableThread: DaemonThreadSummary? {
        if let threadID = store.activeThreadID,
           let summary = store.threads.first(where: { $0.threadID == threadID }) {
            return summary
        }

        if let snapshot = store.activeThreadSnapshot {
            return DaemonThreadSummary(
                threadID: snapshot.threadID,
                title: snapshot.title,
                workingDir: snapshot.workingDir,
                createdAtMS: snapshot.createdAtMS,
                state: "idle",
                participantCount: snapshot.participants.count,
                lastThreadSeq: snapshot.lastThreadSeq
            )
        }

        return nil
    }

    private var connectionStatusColor: Color {
        if store.connectionStatus.contains("Connected") || store.connectionStatus.contains("Synced") {
            return .green
        }
        if store.connectionStatus.contains("Not configured") {
            return .gray
        }
        return .orange
    }

    private func agentStatusColor(for agent: DaemonAgentSummary) -> Color {
        if agent.isOnline {
            return color(named: agent.tintName)
        }
        if agent.isOffline {
            return .secondary
        }
        return .orange
    }

    private func agentSubtitle(for agent: DaemonAgentSummary) -> String {
        if agent.capabilities.isEmpty {
            return agent.kindTitle
        }
        return "\(agent.kindTitle) · \(agent.capabilitySummary)"
    }

    private var canSend: Bool {
        !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func submitDaemonURL() {
        store.updateDaemonURL(daemonURLDraft)
    }

    private func sendDraft() {
        let message = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !message.isEmpty else { return }
        draft = ""
        store.sendCurrentMessage(message)
    }

    private func dismissKeyboard() {
        #if os(iOS)
        UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
        #endif
    }

    private func header(snapshot: DaemonThreadSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .top, spacing: 16) {
                VStack(alignment: .leading, spacing: 10) {
                    Text(snapshot.title ?? snapshot.threadID)
                        .font(.system(.title3, design: .rounded).weight(.medium))
                        .foregroundStyle(ClaudePalette.ink)
                        .lineLimit(2)

                    Label(snapshot.workingDir, systemImage: "folder")
                        .font(.caption)
                        .foregroundStyle(ClaudePalette.mutedInk)
                        .lineLimit(1)
                }

                Spacer(minLength: 0)

                VStack(alignment: .trailing, spacing: 8) {
                    HeaderInfoPill(
                        icon: store.connectionStatus.contains("Connected") || store.connectionStatus.contains("Synced") ? "bolt.fill" : "antenna.radiowaves.left.and.right",
                        text: store.connectionStatus
                    )
                    HeaderInfoPill(icon: "number", text: "Seq \(snapshot.lastThreadSeq)")
                }
            }

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    ForEach(snapshot.participants, id: \.participantID) { participant in
                        ThreadParticipantChip(
                            participant: participant,
                            color: chipColor(for: participant)
                        )
                    }
                }
                .padding(.vertical, 1)
            }
        }
        .frame(maxWidth: 760, alignment: .leading)
        .padding(.horizontal, 22)
        .padding(.vertical, 20)
        .background(
            RoundedRectangle(cornerRadius: 26, style: .continuous)
                .fill(ClaudePalette.panel)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 26, style: .continuous)
                .stroke(ClaudePalette.stroke, lineWidth: 1)
        )
        .shadow(color: Color.black.opacity(0.025), radius: 12, y: 4)
    }

    private func timeline(snapshot: DaemonThreadSnapshot) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    ForEach(store.timeline, id: \.id) { entry in
                        TimelineBubble(entry: entry)
                            .id(entry.id)
                    }
                }
                .frame(maxWidth: 760)
                .padding(.horizontal, 24)
                .padding(.top, 18)
                .padding(.bottom, 160)
                .frame(maxWidth: .infinity)
            }
            .scrollIndicators(.hidden)
            .scrollDismissesKeyboard(.interactively)
            .onTapGesture {
                dismissKeyboard()
            }
            .onChange(of: store.timeline.count) { _ in
                if let lastID = store.timeline.last?.id {
                    withAnimation(.easeOut(duration: 0.22)) {
                        proxy.scrollTo(lastID, anchor: .bottom)
                    }
                }
            }
        }
    }

    private func composer(snapshot: DaemonThreadSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .bottom, spacing: 12) {
                VStack(alignment: .leading, spacing: 10) {
                    TextField("", text: $draft, axis: .vertical)
                        .textFieldStyle(.plain)
                        .lineLimit(1...6)
                        .foregroundStyle(ClaudePalette.ink)
                        .submitLabel(.send)
                        .onSubmit {
                            sendDraft()
                        }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 18)
                .padding(.vertical, 16)
                .background(
                    RoundedRectangle(cornerRadius: 20, style: .continuous)
                        .fill(ClaudePalette.paper)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 20, style: .continuous)
                        .stroke(ClaudePalette.stroke, lineWidth: 1)
                )

                Button(action: sendDraft) {
                    Image(systemName: "arrow.up")
                        .font(.system(size: 17, weight: .bold))
                        .foregroundStyle(canSend ? ClaudePalette.paper : ClaudePalette.subtleInk)
                        .frame(width: 46, height: 46)
                        .background(
                            Circle()
                                .fill(canSend ? ClaudePalette.ink : ClaudePalette.chip)
                        )
                }
                .disabled(!canSend)
            }
        }
        .frame(maxWidth: 760)
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(
            RoundedRectangle(cornerRadius: 26, style: .continuous)
                .fill(ClaudePalette.panel)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 26, style: .continuous)
                .stroke(ClaudePalette.stroke, lineWidth: 1)
        )
        .shadow(color: Color.black.opacity(0.028), radius: 12, y: 4)
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
        color(named: participant.isAgent ? store.tintName(for: participant.agentID) : participant.tintName)
    }

    private func color(named tintName: String) -> Color {
        switch tintName {
        case "purple": return .purple
        case "green": return .green
        case "orange": return .orange
        case "blue": return .blue
        case "gray": return .gray
        case "red": return .red
        default: return .indigo
        }
    }
}

private struct CompactPresentedThread: Identifiable {
    let id: String
}

private enum ClaudePalette {
    static let canvasTop = Color(red: 0.973, green: 0.957, blue: 0.929)
    static let canvasBottom = Color(red: 0.948, green: 0.928, blue: 0.895)
    static let panel = Color(red: 0.981, green: 0.971, blue: 0.949)
    static let paper = Color(red: 0.993, green: 0.988, blue: 0.976)
    static let chip = Color(red: 0.936, green: 0.918, blue: 0.885)
    static let toolPanel = Color(red: 0.957, green: 0.936, blue: 0.892)
    static let planPanel = Color(red: 0.933, green: 0.918, blue: 0.900)
    static let stroke = Color.black.opacity(0.075)
    static let ink = Color(red: 0.200, green: 0.188, blue: 0.173)
    static let mutedInk = Color(red: 0.420, green: 0.392, blue: 0.357)
    static let subtleInk = Color(red: 0.550, green: 0.514, blue: 0.470)
    static let accent = Color(red: 0.694, green: 0.533, blue: 0.333)
}

private struct ChatScreenBackground: View {
    var body: some View {
        ZStack {
            LinearGradient(
                colors: [ClaudePalette.canvasTop, ClaudePalette.canvasBottom],
                startPoint: .top,
                endPoint: .bottom
            )

            Circle()
                .fill(ClaudePalette.accent.opacity(0.10))
                .frame(width: 300, height: 300)
                .blur(radius: 90)
                .offset(x: -160, y: -280)

            Circle()
                .fill(Color.white.opacity(0.38))
                .frame(width: 220, height: 220)
                .blur(radius: 40)
                .offset(x: 170, y: -180)
        }
        .ignoresSafeArea()
    }
}

private struct HeaderInfoPill: View {
    let icon: String
    let text: String

    var body: some View {
        Label(text, systemImage: icon)
            .font(.caption.weight(.medium))
            .foregroundStyle(ClaudePalette.mutedInk)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(ClaudePalette.chip, in: Capsule())
    }
}

private struct ThreadParticipantChip: View {
    let participant: DaemonThreadParticipant
    let color: Color

    var body: some View {
        HStack(spacing: 10) {
            ZStack {
                RoundedRectangle(cornerRadius: 11, style: .continuous)
                    .fill(color.opacity(0.11))
                Text(initials)
                    .font(.caption.weight(.bold))
                    .foregroundStyle(color)
            }
            .frame(width: 30, height: 30)

            VStack(alignment: .leading, spacing: 3) {
                Text(participant.displayName)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(ClaudePalette.ink)
                    .lineLimit(1)
                Text(participant.kindTitle)
                    .font(.caption)
                    .foregroundStyle(ClaudePalette.mutedInk)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(ClaudePalette.paper, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .stroke(ClaudePalette.stroke, lineWidth: 1)
        )
    }

    private var initials: String {
        let parts = participant.displayName.split(separator: " ")
        let value = parts.prefix(2).compactMap { $0.first }.map(String.init).joined()
        return value.isEmpty ? "?" : value.uppercased()
    }
}

private struct TimelineHeroStrip: View {
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

private struct SmallInfoPill: View {
    let icon: String
    let text: String

    var body: some View {
        Label(text, systemImage: icon)
            .font(.caption.weight(.medium))
            .foregroundStyle(ClaudePalette.mutedInk)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(ClaudePalette.paper.opacity(0.95), in: Capsule())
            .overlay(
                Capsule()
                    .stroke(ClaudePalette.stroke, lineWidth: 1)
            )
    }
}

private struct EmptyThreadState: View {
    let snapshot: DaemonThreadSnapshot

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Begin a calm, focused thread")
                .font(.headline)
                .foregroundStyle(ClaudePalette.ink)

            Text("Ask for a summary, a code change, or route work to one or more agents. Responses will unfold here with more room to read.")
                .font(.subheadline)
                .foregroundStyle(ClaudePalette.mutedInk)

            Text(snapshot.participants.map(\.displayName).joined(separator: " · "))
                .font(.caption)
                .foregroundStyle(ClaudePalette.subtleInk)
        }
        .padding(22)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .fill(ClaudePalette.panel)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .stroke(ClaudePalette.stroke, lineWidth: 1)
        )
    }
}

private struct TargetSelectionChip: View {
    let participant: DaemonThreadParticipant
    let color: Color
    let isSelected: Bool

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(isSelected ? ClaudePalette.accent : ClaudePalette.subtleInk)

            Text(participant.displayName)
                .font(.subheadline.weight(.medium))
                .foregroundStyle(ClaudePalette.ink)
                .lineLimit(1)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 9)
        .background(
            RoundedRectangle(cornerRadius: 15, style: .continuous)
                .fill(isSelected ? ClaudePalette.toolPanel : ClaudePalette.paper)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 15, style: .continuous)
                .stroke(isSelected ? ClaudePalette.accent.opacity(0.25) : ClaudePalette.stroke, lineWidth: 1)
        )
    }
}

private struct ThreadFeedRow: View {
    let thread: DaemonThreadSummary
    let isActive: Bool
    let isPinned: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            ZStack {
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .fill(
                        LinearGradient(
                            colors: [Color.accentColor.opacity(0.22), Color.accentColor.opacity(0.10)],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )
                    .frame(width: 56, height: 56)

                Image(systemName: thread.participantCount > 1 ? "person.2.fill" : "message.fill")
                    .font(.system(size: 20, weight: .semibold))
                    .foregroundStyle(Color.accentColor)
            }

            VStack(alignment: .leading, spacing: 8) {
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

                Text(thread.workingDir)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)

                HStack(spacing: 8) {
                    SmallInfoPill(icon: "person.2.fill", text: "\(thread.participantCount)")
                    SmallInfoPill(icon: "number", text: "Seq \(thread.lastThreadSeq)")
                }
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
        Group {
            if entry.kind == .system || entry.kind == .turnEnd {
                centeredEvent
            } else if entry.kind == .user {
                userRow
            } else {
                assistantRow
            }
        }
    }

    private var userRow: some View {
        HStack(alignment: .bottom, spacing: 12) {
            Spacer(minLength: 60)

            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .firstTextBaseline) {
                    Text(entry.title)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.88))

                    Spacer(minLength: 8)

                    Text("seq \(entry.lastThreadSeq)")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(.white.opacity(0.70))
                }

                Text(entry.body.isEmpty ? "…" : entry.body)
                    .font(.body)
                    .foregroundStyle(.white)
                    .lineSpacing(3)
            }
            .padding(.horizontal, 18)
            .padding(.vertical, 16)
            .frame(maxWidth: 420, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .fill(Color(red: 0.274, green: 0.251, blue: 0.228))
            )
            .shadow(color: Color.black.opacity(0.08), radius: 12, y: 4)
        }
    }

    private var assistantRow: some View {
        HStack(alignment: .top, spacing: 14) {
            iconBadge
                .padding(.top, 2)

            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .firstTextBaseline) {
                    Text(entry.title)
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(ClaudePalette.ink)

                    typeTag

                    Spacer(minLength: 8)

                    Text("seq \(entry.lastThreadSeq)")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(ClaudePalette.subtleInk)
                }

                messageBody
            }
            .padding(.horizontal, 18)
            .padding(.vertical, 16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .fill(cardFill)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .stroke(borderColor, lineWidth: 1)
            )

            Spacer(minLength: 0)
        }
    }

    private var messageBody: some View {
        Group {
            if entry.kind == .tool || entry.kind == .plan {
                VStack(alignment: .leading, spacing: 10) {
                    HStack(spacing: 8) {
                        Image(systemName: entry.kind == .tool ? "hammer" : "list.bullet.clipboard")
                            .font(.caption.weight(.semibold))
                        Text(entry.kind == .tool ? "Tool activity" : "Plan draft")
                            .font(.caption.weight(.semibold))
                    }
                    .foregroundStyle(typeColor)

                    Text(entry.body.isEmpty ? "…" : entry.body)
                        .font(.system(.body, design: .monospaced))
                        .foregroundStyle(ClaudePalette.ink)
                        .lineSpacing(4)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(14)
                .background(
                    RoundedRectangle(cornerRadius: 18, style: .continuous)
                        .fill(toolBodyFill)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 18, style: .continuous)
                        .stroke(toolBodyStroke, lineWidth: 1)
                )
            } else if entry.kind == .assistantTurn {
                VStack(alignment: .leading, spacing: 12) {
                    if let thinkingBody = entry.thinkingBody, !thinkingBody.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            Label("Thinking", systemImage: "brain.head.profile")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(ClaudePalette.subtleInk)

                            Text(thinkingBody)
                                .font(.callout)
                                .foregroundStyle(ClaudePalette.subtleInk)
                                .lineSpacing(4)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .padding(14)
                        .background(
                            RoundedRectangle(cornerRadius: 18, style: .continuous)
                                .fill(ClaudePalette.panel.opacity(0.96))
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 18, style: .continuous)
                                .stroke(ClaudePalette.stroke, lineWidth: 1)
                        )
                    }

                    VStack(alignment: .leading, spacing: 8) {
                        if entry.thinkingBody != nil {
                            Label("Response", systemImage: "text.bubble")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(ClaudePalette.mutedInk)
                        }

                        Text(entry.body.isEmpty ? "…" : entry.body)
                            .font(.body)
                            .foregroundStyle(ClaudePalette.ink)
                            .lineSpacing(5)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }

                    if let status = entry.status, status != "completed" {
                        Text(status.replacingOccurrences(of: "_", with: " ").capitalized)
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(ClaudePalette.subtleInk)
                    }
                }
            } else {
                Text(entry.body.isEmpty ? "…" : entry.body)
                    .font(.body)
                    .foregroundStyle(ClaudePalette.ink)
                    .lineSpacing(5)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private var centeredEvent: some View {
        HStack(spacing: 0) {
            Spacer(minLength: 0)

            Label(centeredText, systemImage: iconName)
                .font(.caption.weight(.medium))
                .foregroundStyle(ClaudePalette.mutedInk)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(ClaudePalette.paper, in: Capsule())
                .overlay(
                    Capsule()
                        .stroke(ClaudePalette.stroke, lineWidth: 1)
                )

            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
    }

    private var centeredText: String {
        if entry.body.isEmpty {
            return entry.title
        }
        return "\(entry.title) · \(entry.body)"
    }

    private var typeTag: some View {
        Text(typeLabel)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(typeColor)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(typeTagFill, in: Capsule())
    }

    private var typeLabel: String {
        switch entry.kind {
        case .assistantTurn: return "Assistant"
        case .tool: return "Tool"
        case .plan: return "Plan"
        case .user: return "You"
        case .turnEnd: return "Done"
        case .system: return "System"
        }
    }

    private var iconBadge: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 13, style: .continuous)
                .fill(iconBadgeFill)
            Image(systemName: iconName)
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(typeColor)
        }
        .frame(width: 38, height: 38)
        .overlay(
            RoundedRectangle(cornerRadius: 13, style: .continuous)
                .stroke(ClaudePalette.stroke, lineWidth: 1)
        )
    }

    private var iconName: String {
        switch entry.kind {
        case .user: return "paperplane.fill"
        case .assistantTurn: return "sparkles"
        case .tool: return "wrench.and.screwdriver.fill"
        case .plan: return "list.bullet.clipboard"
        case .turnEnd: return "checkmark.circle"
        case .system: return "sparkles"
        }
    }

    private var typeColor: Color {
        switch entry.kind {
        case .assistantTurn: return ClaudePalette.mutedInk
        case .tool: return ClaudePalette.accent
        case .plan: return Color(red: 0.463, green: 0.392, blue: 0.361)
        case .user: return .white
        case .turnEnd: return ClaudePalette.mutedInk
        case .system: return ClaudePalette.mutedInk
        }
    }

    private var cardFill: Color {
        switch entry.kind {
        case .tool:
            return ClaudePalette.toolPanel
        case .plan:
            return ClaudePalette.planPanel
        case .assistantTurn:
            return ClaudePalette.paper
        case .user, .turnEnd, .system:
            return ClaudePalette.paper
        }
    }

    private var borderColor: Color {
        switch entry.kind {
        case .tool:
            return ClaudePalette.accent.opacity(0.22)
        case .plan:
            return Color(red: 0.463, green: 0.392, blue: 0.361).opacity(0.16)
        case .assistantTurn, .user, .turnEnd, .system:
            return ClaudePalette.stroke
        }
    }

    private var typeTagFill: Color {
        switch entry.kind {
        case .tool:
            return ClaudePalette.paper.opacity(0.65)
        case .plan:
            return ClaudePalette.paper.opacity(0.7)
        default:
            return ClaudePalette.chip.opacity(0.9)
        }
    }

    private var iconBadgeFill: Color {
        switch entry.kind {
        case .tool:
            return ClaudePalette.paper.opacity(0.7)
        case .plan:
            return ClaudePalette.paper.opacity(0.72)
        default:
            return ClaudePalette.paper.opacity(0.96)
        }
    }

    private var toolBodyFill: Color {
        switch entry.kind {
        case .tool:
            return ClaudePalette.paper.opacity(0.68)
        case .plan:
            return ClaudePalette.paper.opacity(0.72)
        default:
            return ClaudePalette.paper
        }
    }

    private var toolBodyStroke: Color {
        switch entry.kind {
        case .tool:
            return ClaudePalette.accent.opacity(0.18)
        case .plan:
            return Color(red: 0.463, green: 0.392, blue: 0.361).opacity(0.14)
        default:
            return ClaudePalette.stroke
        }
    }
}
