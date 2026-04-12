import SwiftUI

struct CommandPaletteView: View {
    @Binding var isPresented: Bool
    @EnvironmentObject private var store: DaemonChatStore
    @FocusState private var isSearchFocused: Bool
    @State private var searchText = ""
    @State private var selectedIndex = 0

    let showNewThreadSheet: () -> Void
    let showAddAgentsSheet: () -> Void
    let toggleInspector: () -> Void
    let focusComposer: () -> Void
    let connectAction: () -> Void

    private var commands: [CommandItem] {
        let allCommands: [CommandItem] = [
            CommandItem(title: "New Thread", shortcut: "⌘N", icon: "plus.bubble") { [self] in
                showNewThreadSheet()
                dismiss()
            },
            CommandItem(title: "Add Agents to Thread", shortcut: "⇧⌘A", icon: "person.badge.plus", requiresThread: true) { [self] in
                showAddAgentsSheet()
                dismiss()
            },
            CommandItem(title: "Reconnect", shortcut: "⇧⌘R", icon: "arrow.clockwise") { [self] in
                store.reconnectNow()
                dismiss()
            },
            CommandItem(title: "Disconnect", shortcut: "", icon: "xmark.circle") { [self] in
                store.disconnect()
                dismiss()
            },
            CommandItem(title: "Toggle Inspector", shortcut: "⌥⌘I", icon: "sidebar.right") { [self] in
                toggleInspector()
                dismiss()
            },
            CommandItem(title: "Focus Composer", shortcut: "⇧⌘L", icon: "text.bubble", requiresThread: true) { [self] in
                focusComposer()
                dismiss()
            },
            CommandItem(title: "Connect", shortcut: "", icon: "link") { [self] in
                connectAction()
                dismiss()
            },
        ]

        if searchText.isEmpty {
            return allCommands
        }

        return allCommands.filter { $0.title.localizedCaseInsensitiveContains(searchText) }
    }

    var body: some View {
        ZStack {
            Color.black.opacity(0.4)
                .ignoresSafeArea()
                .onTapGesture {
                    dismiss()
                }

            VStack(spacing: 0) {
                Spacer()

                VStack(spacing: 0) {
                    Text("Command Palette")
                        .font(.headline)
                        .padding()

                    TextField("Type a command...", text: $searchText)
                        .textFieldStyle(.roundedBorder)
                        .padding(.horizontal)
                        .focused($isSearchFocused)

                    Divider()
                        .padding(.top, 8)

                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 0) {
                            ForEach(Array(commands.enumerated()), id: \.offset) { index, command in
                                CommandRow(
                                    command: command,
                                    isSelected: index == selectedIndex,
                                    hasThread: store.activeThreadID != nil
                                ) {
                                    command.action()
                                }
                                .onTapGesture {
                                    if command.requiresThread && store.activeThreadID == nil {
                                        return
                                    }
                                    command.action()
                                }
                            }
                        }
                    }
                    .frame(maxHeight: 300)
                }
                .background(Color(nsColor: .windowBackgroundColor))
                .cornerRadius(12)
                .shadow(radius: 20)
                .padding(.horizontal, 100)
                .padding(.bottom, 100)
            }
        }
        .onAppear {
            isSearchFocused = true
        }
        .onKeyPress(.upArrow) {
            if selectedIndex > 0 {
                selectedIndex -= 1
            }
            return .handled
        }
        .onKeyPress(.downArrow) {
            if selectedIndex < commands.count - 1 {
                selectedIndex += 1
            }
            return .handled
        }
        .onKeyPress(.return) {
            if selectedIndex < commands.count {
                let command = commands[selectedIndex]
                if !command.requiresThread || store.activeThreadID != nil {
                    command.action()
                }
            }
            return .handled
        }
        .onKeyPress(.escape) {
            dismiss()
            return .handled
        }
        .onChange(of: searchText) { _, _ in
            selectedIndex = 0
        }
    }

    private func dismiss() {
        searchText = ""
        isPresented = false
    }
}

private struct CommandItem: Identifiable {
    let id = UUID()
    let title: String
    let shortcut: String
    let icon: String
    var requiresThread: Bool = false
    let action: () -> Void
}

private struct CommandRow: View {
    let command: CommandItem
    let isSelected: Bool
    let hasThread: Bool
    let onSelect: () -> Void

    private var isDisabled: Bool {
        command.requiresThread && !hasThread
    }

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: command.icon)
                .font(.body)
                .frame(width: 24)
                .foregroundStyle(isDisabled ? .secondary : .primary)

            Text(command.title)
                .font(.body)
                .foregroundStyle(isDisabled ? .secondary : .primary)

            Spacer()

            if !command.shortcut.isEmpty {
                Text(command.shortcut)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(Color.secondary.opacity(0.1), in: RoundedRectangle(cornerRadius: 4))
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(isSelected ? Color.accentColor.opacity(0.2) : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture {
            if !isDisabled {
                onSelect()
            }
        }
    }
}

func desktopTintColor(named name: String) -> Color {
    switch name {
    case "blue":
        return .blue
    case "green":
        return .green
    case "orange":
        return .orange
    case "purple":
        return .purple
    case "red":
        return .red
    default:
        return .gray
    }
}

private struct DesktopSurface<Content: View>: View {
    let content: Content

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    var body: some View {
        content
            .padding(16)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 20, style: .continuous)
                    .stroke(Color.primary.opacity(0.08), lineWidth: 1)
            )
    }
}

private struct DesktopStatusPill: View {
    let label: String
    let tint: Color

    var body: some View {
        Text(label)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(tint)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(tint.opacity(0.14), in: Capsule())
    }
}

private struct InspectorFactRow: View {
    let label: String
    let value: String
    let monospace: Bool

    init(_ label: String, value: String, monospace: Bool = false) {
        self.label = label
        self.value = value
        self.monospace = monospace
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label)
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
            Text(value)
                .font(monospace ? .caption.monospaced() : .callout)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

struct ConnectionStatusCard: View {
    @EnvironmentObject private var store: DaemonChatStore
    @State private var quickConnectURL = ""
    @State private var showQuickConnect = false

    private var presentation: AgentChatDesktopConnectionPresentation {
        AgentChatDesktopConnectionPresentation(state: store.connectionState)
    }

    private var tint: Color {
        desktopTintColor(named: presentation.tintName)
    }

    private var canPasteFromClipboard: Bool {
        if let clipboardString = NSPasteboard.general.string(forType: .string) {
            return clipboardString.contains("agentchat://") ||
                   clipboardString.contains("ws://") ||
                   clipboardString.contains("wss://")
        }
        return false
    }

    var body: some View {
        DesktopSurface {
            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .top, spacing: 12) {
                    ZStack {
                        RoundedRectangle(cornerRadius: 14, style: .continuous)
                            .fill(tint.opacity(0.12))
                            .frame(width: 40, height: 40)
                        Image(systemName: presentation.systemImage)
                            .foregroundStyle(tint)
                            .symbolVariant(.fill)
                    }

                    VStack(alignment: .leading, spacing: 3) {
                        Text("Daemon")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                        Text(presentation.title)
                            .font(.headline)
                        Text(presentation.subtitle)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    Spacer()

                    if presentation.isWorking {
                        ProgressView()
                            .controlSize(.small)
                    } else {
                        DesktopStatusPill(
                            label: store.connectionState.isOnline ? "Live" : "Idle",
                            tint: tint
                        )
                    }
                }

                if showQuickConnect && !store.connectionState.isOnline {
                    quickConnectSection
                } else if !store.daemonURL.isEmpty {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Endpoint")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.secondary)
                        Text(store.daemonURL)
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                            .lineLimit(3)
                            .textSelection(.enabled)
                    }
                }

                if let errorSummary = store.desktopConnectionErrorSummary {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(alignment: .top, spacing: 8) {
                            Image(systemName: store.desktopHasConnectionIssue ? "exclamationmark.triangle.fill" : "info.circle.fill")
                                .foregroundStyle(store.desktopHasConnectionIssue ? Color.orange : tint)
                            Text(errorSummary)
                                .font(.caption)
                                .foregroundStyle(store.desktopHasConnectionIssue ? Color.orange : .secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }

                        if let errorDetail = store.desktopConnectionErrorDetail {
                            DisclosureGroup("Connection details") {
                                Text(errorDetail)
                                    .font(.caption.monospaced())
                                    .foregroundStyle(.secondary)
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(.top, 4)
                            }
                            .font(.caption)
                        }
                    }
                }

                HStack(spacing: 8) {
                    if !store.connectionState.isOnline {
                        Button(showQuickConnect ? "Hide" : "Connect") {
                            withAnimation(.easeInOut(duration: 0.2)) {
                                showQuickConnect.toggle()
                            }
                        }
                        .buttonStyle(.bordered)
                    }

                    Button("Reconnect") {
                        store.reconnectNow()
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(!store.hasConfiguredDaemonURL)

                    Button("Disconnect") {
                        store.disconnect()
                    }
                    .buttonStyle(.bordered)
                    .disabled(!store.hasConfiguredDaemonURL)
                }
            }
        }
    }

    private var quickConnectSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            TextField("ws://127.0.0.1:9390 or agentchat://...", text: $quickConnectURL)
                .textFieldStyle(.roundedBorder)
                .font(.caption)
                .onSubmit {
                    connectWithURL()
                }

            HStack(spacing: 8) {
                Button("Paste & Connect") {
                    if let clipboardString = NSPasteboard.general.string(forType: .string) {
                        quickConnectURL = clipboardString.trimmingCharacters(in: .whitespacesAndNewlines)
                        connectWithURL()
                    }
                }
                .buttonStyle(.bordered)
                .disabled(!canPasteFromClipboard)
                .help("Paste URL from clipboard")

                Button("Apply") {
                    connectWithURL()
                }
                .buttonStyle(.borderedProminent)
                .disabled(quickConnectURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

                Spacer()

                Text("Press Enter to connect")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }

            Text("Supports ws://, wss://, or agentchat://connect? links")
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding(8)
        .background(Color.black.opacity(0.03), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        .onAppear {
            if let clipboardString = NSPasteboard.general.string(forType: .string),
               canPasteFromClipboard {
                quickConnectURL = clipboardString.trimmingCharacters(in: .whitespacesAndNewlines)
            }
        }
    }

    private func connectWithURL() {
        let trimmedURL = quickConnectURL.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedURL.isEmpty else { return }

        store.updateDaemonURL(trimmedURL)
        quickConnectURL = ""
        showQuickConnect = false
    }
}

struct ThreadSidebarRow: View {
    let thread: DaemonThreadSummary
    let title: String
    let isSelected: Bool
    let isActive: Bool
    let isPinned: Bool
    let onPin: () -> Void
    let onHide: () -> Void
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .center, spacing: 8) {
                Circle()
                    .fill(isActive ? Color.green : Color.secondary.opacity(0.35))
                    .frame(width: 8, height: 8)

                Text(title)
                    .font(.headline)
                    .lineLimit(1)

                Spacer()

                if isPinned {
                    Image(systemName: "pin.fill")
                        .font(.caption)
                        .foregroundStyle(.orange)
                }

                DesktopStatusPill(label: thread.state.capitalized, tint: isActive ? .green : .secondary)
            }

            HStack(spacing: 10) {
                Label("\(thread.participantCount)", systemImage: "person.2")
                Label("Seq \(thread.lastThreadSeq)", systemImage: "arrow.up.message")
            }
            .font(.caption)
            .foregroundStyle(.secondary)

            Text(thread.workingDir)
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(isSelected ? Color.accentColor.opacity(0.15) : Color.clear)
        )
        .contextMenu {
            Button {
                onPin()
            } label: {
                Label(isPinned ? "Unpin" : "Pin", systemImage: isPinned ? "pin.slash" : "pin")
            }

            Button {
                onHide()
            } label: {
                Label("Hide", systemImage: "eye.slash")
            }

            Divider()

            Button(role: .destructive) {
                onClose()
            } label: {
                Label("Close Thread", systemImage: "xmark.circle")
            }
        }
    }
}

struct AgentSidebarRow: View {
    let agent: DaemonAgentSummary
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                ZStack {
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .fill(desktopTintColor(named: agent.tintName).opacity(0.15))
                        .frame(width: 34, height: 34)
                    Image(systemName: agent.symbolName)
                        .foregroundStyle(desktopTintColor(named: agent.tintName))
                }

                VStack(alignment: .leading, spacing: 2) {
                    Text(agent.displayName)
                        .font(.body.weight(.medium))
                    Text(agent.capabilitySummary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                Spacer()

                if agent.isOnline {
                    Image(systemName: "dot.radiowaves.left.and.right")
                        .foregroundStyle(.green)
                }
            }
        }
        .buttonStyle(.plain)
    }
}

struct ThreadDetailView: View {
    let thread: DaemonThreadSummary
    let snapshot: DaemonThreadSnapshot?
    let timeline: [DaemonTimelineEntry]
    let connectionState: DaemonConnectionState
    let isLoadingThreadContent: Bool
    @Binding var composerText: String
    @Binding var selectedParticipantIDs: Set<String>
    @FocusState.Binding var isComposerFocused: Bool
    let onSend: () -> Void

    private var titleText: String {
        snapshot?.title ?? thread.title ?? thread.threadID
    }

    private var agentParticipants: [DaemonThreadParticipant] {
        (snapshot?.participants ?? []).filter(\.isAgent)
    }

    private var trimmedComposerText: String {
        composerText.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var canSend: Bool {
        !trimmedComposerText.isEmpty && !agentParticipants.isEmpty && !selectedParticipantIDs.isEmpty
    }

    private var composerHint: String {
        if isLoadingThreadContent {
            return "Waiting for thread data to finish syncing."
        }
        if agentParticipants.isEmpty {
            return "Add at least one agent to this thread before sending."
        }
        if selectedParticipantIDs.isEmpty {
            return "Choose at least one target agent above before sending."
        }
        return "Send with Command-Return"
    }

    var body: some View {
        VStack(spacing: 18) {
            DesktopSurface {
                HStack(alignment: .top, spacing: 16) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(titleText)
                            .font(.title2.weight(.bold))
                        Text(thread.workingDir)
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                            .textSelection(.enabled)

                        HStack(spacing: 8) {
                            DesktopStatusPill(label: thread.state.capitalized, tint: .secondary)
                            Label("\(thread.participantCount) participants", systemImage: "person.2")
                            Label("Seq \(thread.lastThreadSeq)", systemImage: "arrow.up.forward")
                        }
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    }

                    Spacer()

                    if isLoadingThreadContent {
                        ProgressView()
                            .controlSize(.small)
                    } else if connectionState.isOnline {
                        DesktopStatusPill(label: "Live", tint: .green)
                    }
                }
            }

            DesktopSurface {
                VStack(alignment: .leading, spacing: 14) {
                    if let snapshot {
                        ParticipantPicker(participants: snapshot.participants, selectedParticipantIDs: $selectedParticipantIDs)
                    }

                    if isLoadingThreadContent {
                        ThreadDetailLoadingStateView(statusText: connectionState.statusText)
                    } else if timeline.isEmpty {
                        ContentUnavailableView(
                            "No Activity Yet",
                            systemImage: "ellipsis.message",
                            description: Text("Messages, agent deltas, tools, and plans will appear here as this thread runs.")
                        )
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    } else {
                        TimelineListView(timeline: timeline)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)

            DesktopSurface {
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        Text("Composer")
                            .font(.headline)
                        Spacer()
                        if !agentParticipants.isEmpty {
                            Text("\(selectedParticipantIDs.count) of \(agentParticipants.count) targets")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }

                    ZStack(alignment: .topLeading) {
                        TextEditor(text: $composerText)
                            .font(.body)
                            .focused($isComposerFocused)
                            .frame(minHeight: 112)
                            .padding(10)
                            .background(
                                RoundedRectangle(cornerRadius: 14, style: .continuous)
                                    .fill(Color.black.opacity(0.03))
                            )
                            .overlay(
                                RoundedRectangle(cornerRadius: 14, style: .continuous)
                                    .stroke(
                                        isComposerFocused ? Color.accentColor.opacity(0.55) : Color.primary.opacity(0.06),
                                        lineWidth: isComposerFocused ? 1.5 : 1
                                    )
                            )

                        if composerText.isEmpty {
                            Text("Message the selected agents...")
                                .foregroundStyle(.tertiary)
                                .padding(.horizontal, 16)
                                .padding(.vertical, 18)
                                .allowsHitTesting(false)
                        }
                    }

                    HStack(alignment: .firstTextBaseline) {
                        Text(composerHint)
                            .font(.caption)
                            .foregroundStyle(selectedParticipantIDs.isEmpty && !agentParticipants.isEmpty ? Color.orange : .secondary)

                        Spacer()

                        Button("Send", action: onSend)
                            .keyboardShortcut(.return, modifiers: [.command])
                            .buttonStyle(.borderedProminent)
                            .disabled(!canSend)
                    }
                }
            }
        }
        .padding(20)
    }
}

private struct ThreadDetailLoadingStateView: View {
    let statusText: String

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                ProgressView()
                    .controlSize(.small)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Loading thread")
                        .font(.headline)
                    Text(statusText)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            ForEach(0..<3, id: \.self) { _ in
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(Color.secondary.opacity(0.09))
                    .frame(height: 74)
                    .overlay(alignment: .topLeading) {
                        VStack(alignment: .leading, spacing: 8) {
                            RoundedRectangle(cornerRadius: 4, style: .continuous)
                                .fill(Color.secondary.opacity(0.16))
                                .frame(width: 160, height: 10)
                            RoundedRectangle(cornerRadius: 4, style: .continuous)
                                .fill(Color.secondary.opacity(0.12))
                                .frame(maxWidth: .infinity)
                                .frame(height: 10)
                            RoundedRectangle(cornerRadius: 4, style: .continuous)
                                .fill(Color.secondary.opacity(0.1))
                                .frame(width: 220, height: 10)
                        }
                        .padding(16)
                    }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

struct TimelineListView: View {
    let timeline: [DaemonTimelineEntry]

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    ForEach(timeline) { entry in
                        TimelineEntryCard(entry: entry)
                            .id(entry.id)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .onAppear {
                scrollToBottom(using: proxy)
            }
            .onChange(of: timeline.count) { _, _ in
                scrollToBottom(using: proxy)
            }
        }
    }

    private func scrollToBottom(using proxy: ScrollViewProxy) {
        guard let lastID = timeline.last?.id else { return }
        DispatchQueue.main.async {
            withAnimation(.easeOut(duration: 0.18)) {
                proxy.scrollTo(lastID, anchor: .bottom)
            }
        }
    }
}

struct TimelineEntryCard: View {
    let entry: DaemonTimelineEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .center) {
                Label(entry.title, systemImage: symbolName)
                    .font(.headline)
                    .foregroundStyle(desktopTintColor(named: entry.tintName))

                Spacer()

                if let status = entry.status, !status.isEmpty {
                    Text(status.capitalized)
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.secondary)
                }
            }

            if !entry.body.isEmpty {
                Text(entry.body)
                    .font(.body)
                    .textSelection(.enabled)
            }

            if let thinkingBody = entry.thinkingBody, !thinkingBody.isEmpty {
                DisclosureGroup("Thinking") {
                    Text(thinkingBody)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.top, 4)
                }
            }

            if let planBody = entry.planBody, !planBody.isEmpty {
                DisclosureGroup("Plan") {
                    Text(planBody)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.top, 4)
                }
            }

            if !entry.toolActivities.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Tools")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    ForEach(entry.orderedToolActivities) { activity in
                        HStack {
                            Image(systemName: activitySymbolName(activity))
                                .foregroundStyle(desktopTintColor(named: entry.tintName))
                            VStack(alignment: .leading, spacing: 2) {
                                Text(activity.displayTitle)
                                    .font(.callout.weight(.medium))
                                Text(activity.displayStatus)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                        }
                    }
                }
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .fill(desktopTintColor(named: entry.tintName).opacity(0.08))
        )
    }

    private var symbolName: String {
        switch entry.kind {
        case .user:
            return "person.fill"
        case .assistantTurn:
            return "sparkles.rectangle.stack"
        case .tool:
            return "hammer.fill"
        case .plan:
            return "list.bullet.clipboard"
        case .turnEnd:
            return "checkmark.circle"
        case .system:
            return "info.circle"
        }
    }

    private func activitySymbolName(_ activity: DaemonToolActivity) -> String {
        if activity.needsApproval {
            return "hand.raised.fill"
        }
        if activity.isFailed {
            return "xmark.octagon.fill"
        }
        if activity.isRunning {
            return "ellipsis.circle.fill"
        }
        return "checkmark.circle.fill"
    }
}

struct ParticipantPicker: View {
    let participants: [DaemonThreadParticipant]
    @Binding var selectedParticipantIDs: Set<String>

    private var agentParticipants: [DaemonThreadParticipant] {
        participants.filter(\.isAgent)
    }

    private var allAgentIDs: Set<String> {
        Set(agentParticipants.map(\.participantID))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Targets")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)

                Spacer()

                if !agentParticipants.isEmpty && selectedParticipantIDs != allAgentIDs {
                    Button("Select All") {
                        selectedParticipantIDs = allAgentIDs
                    }
                    .buttonStyle(.plain)
                    .font(.caption)
                }
            }

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(agentParticipants) { participant in
                        let isSelected = selectedParticipantIDs.contains(participant.participantID)
                        Button {
                            if isSelected {
                                selectedParticipantIDs.remove(participant.participantID)
                            } else {
                                selectedParticipantIDs.insert(participant.participantID)
                            }
                        } label: {
                            Label(participant.displayName, systemImage: participant.family.symbolName)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 8)
                                .background(
                                    Capsule()
                                        .fill(
                                            isSelected
                                                ? desktopTintColor(named: participant.tintName).opacity(0.18)
                                                : Color.secondary.opacity(0.08)
                                        )
                                )
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }
}

struct ThreadInspectorView: View {
    let thread: DaemonThreadSummary?
    let snapshot: DaemonThreadSnapshot?
    let participants: [DaemonThreadParticipant]
    let selectedParticipantIDs: Set<String>
    @Binding var showAddAgentsSheet: Bool

    private var selectedParticipants: [DaemonThreadParticipant] {
        participants.filter { selectedParticipantIDs.contains($0.participantID) }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                DesktopSurface {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack {
                            Text("Thread Inspector")
                                .font(.headline)
                            Spacer()
                            Button("Add Agents") {
                                showAddAgentsSheet = true
                            }
                            .disabled(thread == nil)
                        }

                        if let thread {
                            Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 10) {
                                GridRow {
                                    InspectorFactRow("Thread ID", value: thread.threadID, monospace: true)
                                    InspectorFactRow("Created", value: thread.createdAtDate.formatted(date: .abbreviated, time: .shortened))
                                }
                                GridRow {
                                    InspectorFactRow("Working Dir", value: snapshot?.workingDir ?? thread.workingDir, monospace: true)
                                    InspectorFactRow("State", value: thread.state.capitalized)
                                }
                                GridRow {
                                    InspectorFactRow("Participants", value: "\(participants.count)")
                                    InspectorFactRow("Last Seq", value: "\(snapshot?.lastThreadSeq ?? thread.lastThreadSeq)")
                                }
                            }

                            if !selectedParticipants.isEmpty {
                                VStack(alignment: .leading, spacing: 6) {
                                    Text("Current Targets")
                                        .font(.caption.weight(.semibold))
                                        .foregroundStyle(.secondary)
                                    ScrollView(.horizontal, showsIndicators: false) {
                                        HStack(spacing: 8) {
                                            ForEach(selectedParticipants) { participant in
                                                DesktopStatusPill(
                                                    label: participant.displayName,
                                                    tint: desktopTintColor(named: participant.tintName)
                                                )
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            Text("No thread selected")
                                .foregroundStyle(.secondary)
                        }
                    }
                }

                DesktopSurface {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Participants")
                            .font(.headline)

                        if participants.isEmpty {
                            Text("Open a thread to inspect who is participating.")
                                .foregroundStyle(.secondary)
                        } else {
                            ForEach(participants) { participant in
                                VStack(alignment: .leading, spacing: 6) {
                                    HStack(alignment: .center, spacing: 10) {
                                        Image(systemName: participant.family.symbolName)
                                            .foregroundStyle(desktopTintColor(named: participant.tintName))
                                        VStack(alignment: .leading, spacing: 2) {
                                            HStack(spacing: 6) {
                                                Text(participant.displayName)
                                                    .font(.body.weight(.medium))
                                                if selectedParticipantIDs.contains(participant.participantID) {
                                                    DesktopStatusPill(label: "Targeted", tint: desktopTintColor(named: participant.tintName))
                                                }
                                            }
                                            Text(participant.kindTitle)
                                                .font(.caption)
                                                .foregroundStyle(.secondary)
                                        }
                                        Spacer()
                                        DesktopStatusPill(label: participant.state.capitalized, tint: .secondary)
                                    }

                                    Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 6) {
                                        GridRow {
                                            InspectorFactRow("Mention", value: participant.mentionHandle, monospace: true)
                                            InspectorFactRow("Participant ID", value: participant.participantID, monospace: true)
                                        }
                                        if let agentID = participant.agentID {
                                            GridRow {
                                                InspectorFactRow("Agent ID", value: agentID, monospace: true)
                                                InspectorFactRow("Session", value: participant.sessionID ?? "Unavailable", monospace: true)
                                            }
                                        } else if let sessionID = participant.sessionID {
                                            GridRow {
                                                InspectorFactRow("Session", value: sessionID, monospace: true)
                                                InspectorFactRow("Kind", value: participant.kind)
                                            }
                                        }
                                    }
                                }
                                .padding(.vertical, 2)
                            }
                        }
                    }
                }

                if let snapshot {
                    DesktopSurface {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Snapshot")
                                .font(.headline)
                            Text(snapshot.title ?? "Untitled thread")
                                .font(.body.weight(.medium))
                            Text("Snapshot working dir")
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(.secondary)
                            Text(snapshot.workingDir)
                                .font(.caption.monospaced())
                                .foregroundStyle(.secondary)
                                .textSelection(.enabled)
                        }
                    }
                }
            }
            .padding(20)
        }
    }
}

struct AgentSelectionSheet: View {
    @Environment(\.dismiss) private var dismiss
    @EnvironmentObject private var store: DaemonChatStore

    let title: String
    let subtitle: String
    let agents: [DaemonAgentSummary]
    let initiallySelected: Set<String>
    let confirmLabel: String
    let onConfirm: ([String]) -> Void

    @State private var selection: Set<String> = []

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(title)
                .font(.title2.weight(.bold))
            Text(subtitle)
                .foregroundStyle(.secondary)

            List {
                ForEach(agents, id: \.agentID) { agent in
                    let isSelected = selection.contains(agent.agentID)
                    Button {
                        if isSelected {
                            selection.remove(agent.agentID)
                        } else {
                            selection.insert(agent.agentID)
                        }
                    } label: {
                        HStack(spacing: 10) {
                            Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                                .foregroundStyle(isSelected ? Color.accentColor : Color.secondary)
                            Image(systemName: agent.symbolName)
                                .foregroundStyle(desktopTintColor(named: agent.tintName))
                            VStack(alignment: .leading, spacing: 2) {
                                Text(agent.displayName)
                                    .font(.body.weight(.medium))
                                Text(agent.capabilitySummary)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            if store.selectedAgentIDs.contains(agent.agentID) {
                                Text("Recent")
                                    .font(.caption2.weight(.semibold))
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                    .buttonStyle(.plain)
                }
            }
            .frame(minHeight: 280)

            HStack {
                Button("Cancel", role: .cancel) {
                    dismiss()
                }
                Spacer()
                Button(confirmLabel) {
                    onConfirm(Array(selection).sorted())
                    dismiss()
                }
                .buttonStyle(.borderedProminent)
                .disabled(selection.isEmpty)
            }
        }
        .padding(24)
        .frame(width: 520, height: 480)
        .onAppear {
            selection = initiallySelected.intersection(Set(agents.map(\.agentID)))
        }
    }
}
