import SwiftUI

struct CommandPaletteView: View {
    @Binding var isPresented: Bool
    @EnvironmentObject private var store: DaemonChatStore
    @FocusState private var isSearchFocused: Bool
    @State private var searchText = ""
    @State private var selectedIndex = 0

    let showNewThreadSheet: () -> Void
    let showAddAgentsSheet: () -> Void
    let focusComposer: () -> Void
    let connectAction: () -> Void

    private var commands: [CommandItem] {
        var allCommands: [CommandItem] = [
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
            CommandItem(title: "Focus Composer", shortcut: "⇧⌘L", icon: "text.bubble", requiresThread: true) { [self] in
                focusComposer()
                dismiss()
            },
        ]

        if !store.connectionState.isOnline {
            allCommands.append(
                CommandItem(title: "Connect", shortcut: "", icon: "link") { [self] in
                connectAction()
                dismiss()
                }
            )
        }

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
    var padding: CGFloat = 16
    let content: Content

    init(padding: CGFloat = 16, @ViewBuilder content: () -> Content) {
        self.padding = padding
        self.content = content()
    }

    var body: some View {
        content
            .padding(padding)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 20, style: .continuous)
                    .stroke(Color.primary.opacity(0.08), lineWidth: 1)
            )
    }
}

private struct DesktopHeroSurface<Content: View>: View {
    let tint: Color
    let content: Content

    init(tint: Color, @ViewBuilder content: () -> Content) {
        self.tint = tint
        self.content = content()
    }

    var body: some View {
        content
            .padding(22)
            .background(
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .fill(.regularMaterial)
                    .overlay(
                        LinearGradient(
                            colors: [
                                tint.opacity(0.16),
                                Color.clear,
                            ],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                        .clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
                    )
            )
            .overlay(
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .stroke(tint.opacity(0.16), lineWidth: 1)
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

struct SidebarOverviewCard: View {
    let threadCount: Int
    let onlineAgentCount: Int
    let isOnline: Bool

    var body: some View {
        DesktopHeroSurface(tint: isOnline ? .green : .orange) {
            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("AgentChat")
                            .font(.title3.weight(.semibold))
                        Text("A calmer workspace for daemon-backed agent threads.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    Spacer()

                    DesktopStatusPill(label: isOnline ? "Live" : "Offline", tint: isOnline ? .green : .orange)
                }

                HStack(spacing: 10) {
                    sidebarMetric(label: "Threads", value: "\(threadCount)")
                    sidebarMetric(label: "Online", value: "\(onlineAgentCount)")
                }
            }
        }
    }

    private func sidebarMetric(label: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(value)
                .font(.headline.weight(.semibold))
            Text(label.uppercased())
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
                .tracking(0.6)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
        .background(Color(nsColor: .controlBackgroundColor).opacity(0.18), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
}

struct OnlineAgentsPanel: View {
    let agents: [DaemonAgentSummary]
    let onConnect: (DaemonAgentSummary) -> Void
    let onEdit: (DaemonAgentSummary) -> Void

    var body: some View {
        DesktopSurface {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("Online Agents")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Spacer()
                    Text("\(agents.count)")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.secondary)
                }

                if agents.isEmpty {
                    Text("Reconnect to load available agents.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    VStack(spacing: 8) {
                        ForEach(agents) { agent in
                            HStack(spacing: 10) {
                                ZStack {
                                    RoundedRectangle(cornerRadius: 9, style: .continuous)
                                        .fill(desktopTintColor(named: agent.tintName).opacity(0.14))
                                        .frame(width: 28, height: 28)
                                    Image(systemName: agent.symbolName)
                                        .font(.caption)
                                        .foregroundStyle(desktopTintColor(named: agent.tintName))
                                }

                                VStack(alignment: .leading, spacing: 1) {
                                    Text(agent.displayName)
                                        .font(.callout.weight(.medium))
                                        .lineLimit(1)
                                    Text(agent.capabilitySummary)
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                }

                                Spacer()

                                Button {
                                    onEdit(agent)
                                } label: {
                                    Image(systemName: "slider.horizontal.3")
                                }
                                .buttonStyle(.plain)
                                .foregroundStyle(.secondary)

                                Button("Open") {
                                    onConnect(agent)
                                }
                                .buttonStyle(.bordered)
                                .controlSize(.small)
                            }
                        }
                    }
                }
            }
        }
    }
}

struct WorkspaceStartView: View {
    let connectionState: DaemonConnectionState
    let hasConfiguredDaemonURL: Bool
    let onlineAgentCount: Int
    let threadCount: Int
    let onNewThread: () -> Void
    let onReconnect: () -> Void
    let onConnect: () -> Void

    private var isOnline: Bool {
        connectionState.isOnline
    }

    var body: some View {
        VStack {
            Spacer(minLength: 32)

            DesktopHeroSurface(tint: isOnline ? .green : .accentColor) {
                VStack(alignment: .leading, spacing: 24) {
                    HStack(alignment: .top, spacing: 18) {
                        ZStack {
                            Circle()
                                .fill((isOnline ? Color.green : Color.accentColor).opacity(0.14))
                                .frame(width: 54, height: 54)
                            Image(systemName: "bubble.left.and.text.bubble.right.fill")
                                .font(.title2)
                                .foregroundStyle(isOnline ? Color.green : Color.accentColor)
                        }

                        VStack(alignment: .leading, spacing: 8) {
                            Text("Start Building")
                                .font(.system(size: 34, weight: .semibold, design: .rounded))
                            Text(startMessage)
                                .font(.title3)
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }

                    HStack(spacing: 12) {
                        Button("New Thread", action: onNewThread)
                            .buttonStyle(.borderedProminent)

                        if isOnline || hasConfiguredDaemonURL {
                            Button("Reconnect", action: onReconnect)
                                .buttonStyle(.bordered)
                        } else {
                            Button("Connect Daemon", action: onConnect)
                                .buttonStyle(.bordered)
                        }
                    }

                    HStack(spacing: 12) {
                        startFact(label: "Threads", value: "\(threadCount)", icon: "bubble.left.and.bubble.right")
                        startFact(label: "Online Agents", value: "\(onlineAgentCount)", icon: "bolt.horizontal.circle")
                        startFact(label: "State", value: connectionState.statusText, icon: "waveform.path.ecg")
                    }

                    VStack(alignment: .leading, spacing: 10) {
                        Text("Prompt ideas")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)

                        HStack(spacing: 10) {
                            suggestionCard("Review the current daemon connection flow.")
                            suggestionCard("Open a thread and ask two agents to compare approaches.")
                            suggestionCard("Reconnect to the daemon and resume the latest workspace.")
                        }
                    }
                }
            }
            .frame(maxWidth: 860)

            Spacer()
        }
        .padding(28)
    }

    private var startMessage: String {
        if isOnline {
            return "Open a thread from the sidebar or start a new one from here."
        }
        if hasConfiguredDaemonURL {
            return "Reconnect to your daemon, then jump back into a live thread."
        }
        return "Connect a daemon first, then start a new agent conversation."
    }

    private func startFact(label: String, value: String, icon: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: icon)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 2) {
                Text(label)
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
                Text(value)
                    .font(.callout.weight(.medium))
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
        .padding(12)
        .background(Color(nsColor: .controlBackgroundColor).opacity(0.18), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }

    private func suggestionCard(_ text: String) -> some View {
        Text(text)
            .font(.callout)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(14)
            .background(Color(nsColor: .controlBackgroundColor).opacity(0.16), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
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

private struct InspectorSection<Content: View>: View {
    let title: String
    let content: Content

    init(_ title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .tracking(0.4)
            content
        }
        .padding(.vertical, 8)
    }
}

private struct DesktopComposerMentionContext: Equatable {
    let replacementRange: Range<String.Index>
    let query: String
}

private struct DesktopSlashContext: Equatable {
    let replacementRange: Range<String.Index>
    let query: String
}

private struct DesktopSlashCommand: Identifiable {
    let command: String
    let template: String
    let title: String
    let description: String

    var id: String { command }
}

private enum DesktopComposerAssistantItem: Identifiable {
    case mention(DaemonThreadParticipant)
    case slash(DesktopSlashCommand)

    var id: String {
        switch self {
        case .mention(let participant):
            return "mention-\(participant.participantID)"
        case .slash(let command):
            return "slash-\(command.id)"
        }
    }
}

private func desktopActiveComposerMentionContext(in text: String) -> DesktopComposerMentionContext? {
    let end = text.endIndex
    guard !text.isEmpty else { return nil }

    let contentEnd = text[..<end].lastIndex(where: { !$0.isWhitespace }).map { text.index(after: $0) } ?? text.startIndex
    if contentEnd == text.startIndex {
        return nil
    }

    var tokenStart = contentEnd
    while tokenStart > text.startIndex {
        let previousIndex = text.index(before: tokenStart)
        if !desktopIsMentionCharacter(text[previousIndex]) {
            break
        }
        tokenStart = previousIndex
    }

    guard tokenStart > text.startIndex else { return nil }
    let mentionStart = text.index(before: tokenStart)
    guard text[mentionStart] == "@",
          desktopIsMentionLeadingBoundary(mentionStart > text.startIndex ? text[text.index(before: mentionStart)] : nil)
    else {
        return nil
    }

    return DesktopComposerMentionContext(
        replacementRange: mentionStart..<end,
        query: String(text[tokenStart..<contentEnd])
    )
}

private func desktopActiveSlashContext(in text: String) -> DesktopSlashContext? {
    let end = text.endIndex
    guard !text.isEmpty else { return nil }

    let contentEnd = text[..<end].lastIndex(where: { !$0.isWhitespace }).map { text.index(after: $0) } ?? text.startIndex
    if contentEnd == text.startIndex {
        return nil
    }

    var tokenStart = contentEnd
    while tokenStart > text.startIndex {
        let previousIndex = text.index(before: tokenStart)
        if !desktopIsSlashCharacter(text[previousIndex]) {
            break
        }
        tokenStart = previousIndex
    }

    let slashStart = tokenStart > text.startIndex ? text.index(before: tokenStart) : text.startIndex
    guard slashStart < end,
          text[slashStart] == "/",
          desktopIsMentionLeadingBoundary(slashStart > text.startIndex ? text[text.index(before: slashStart)] : nil)
    else {
        return nil
    }

    return DesktopSlashContext(
        replacementRange: slashStart..<end,
        query: String(text[tokenStart..<contentEnd])
    )
}

private func desktopIsMentionCharacter(_ character: Character) -> Bool {
    character.isLetter || character.isNumber || character == "-" || character == "_" || character == "."
}

private func desktopIsSlashCharacter(_ character: Character) -> Bool {
    character.isLetter || character.isNumber || character == "-" || character == "_"
}

private func desktopIsMentionLeadingBoundary(_ character: Character?) -> Bool {
    guard let character else { return true }
    if character.isWhitespace {
        return true
    }

    switch character {
    case "(", "[", "{", "\"", "'", "，", "。", ",", ":", ";":
        return true
    default:
        return false
    }
}

private func desktopThreadStateTint(_ state: String, accent: Color = .accentColor) -> Color {
    let normalized = state.lowercased()
    if normalized.contains("run") || normalized.contains("stream") || normalized.contains("busy") {
        return accent
    }
    if normalized.contains("error") || normalized.contains("fail") {
        return .red
    }
    if normalized.contains("wait") || normalized.contains("input") || normalized.contains("pending") {
        return .orange
    }
    return .secondary
}

private func desktopThreadShowsActivity(_ state: String) -> Bool {
    let normalized = state.lowercased()
    return normalized.contains("run") || normalized.contains("stream") || normalized.contains("busy")
}

struct ConnectionStatusCard: View {
    @EnvironmentObject private var store: DaemonChatStore
    @Binding var showQuickConnect: Bool
    @State private var quickConnectURL = ""

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
                headerSection

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

                actionSection
            }
        }
    }

    @ViewBuilder
    private var headerSection: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .top, spacing: 12) {
                connectionIcon

                VStack(alignment: .leading, spacing: 3) {
                    Text(presentation.title)
                        .font(.headline.weight(.semibold))
                        .lineLimit(2)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(presentation.subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                connectionStatusAccessory
            }

            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .top, spacing: 12) {
                    connectionIcon

                    VStack(alignment: .leading, spacing: 3) {
                        Text(presentation.title)
                            .font(.headline.weight(.semibold))
                            .lineLimit(2)
                            .fixedSize(horizontal: false, vertical: true)
                        Text(presentation.subtitle)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                connectionStatusAccessory
            }
        }
    }

    private var connectionIcon: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(tint.opacity(0.12))
                .frame(width: 40, height: 40)
            Image(systemName: presentation.systemImage)
                .foregroundStyle(tint)
                .symbolVariant(.fill)
        }
    }

    @ViewBuilder
    private var connectionStatusAccessory: some View {
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

    @ViewBuilder
    private var actionSection: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 8) {
                actionButtons
            }

            VStack(alignment: .leading, spacing: 8) {
                actionButtons
            }
        }
    }

    @ViewBuilder
    private var actionButtons: some View {
        if !store.connectionState.isOnline {
            Button(showQuickConnect ? "Hide" : "Connect") {
                withAnimation(.easeInOut(duration: 0.2)) {
                    showQuickConnect.toggle()
                }
            }
            .buttonStyle(.bordered)
        }

        if store.connectionState.isOnline {
            Button("Reconnect") {
                store.reconnectNow()
            }
            .buttonStyle(.bordered)
            .disabled(!store.hasConfiguredDaemonURL)
        } else {
            Button("Reconnect") {
                store.reconnectNow()
            }
            .buttonStyle(.borderedProminent)
            .disabled(!store.hasConfiguredDaemonURL)
        }

        Button("Disconnect") {
            store.disconnect()
        }
        .buttonStyle(.bordered)
        .disabled(!store.hasConfiguredDaemonURL)
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
    let preview: String?
    let isSelected: Bool
    let isActive: Bool
    let isUnread: Bool
    let isPinned: Bool
    let onPin: () -> Void
    let onHide: () -> Void
    let onClose: () -> Void
    let onOpenInNewWindow: () -> Void
    let snapshot: DaemonThreadSnapshot?
    let participants: [DaemonThreadParticipant]
    let onShowAddAgents: () -> Void
    @State private var isHovering = false
    @State private var showThreadDetail = false

    private var relativeDateText: String {
        thread.createdAtDate.formatted(.relative(presentation: .named))
    }

    private var secondaryText: String {
        let trimmedPreview = preview?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !trimmedPreview.isEmpty {
            return trimmedPreview
        }
        return thread.workingDir
    }

    private var stateTint: Color {
        desktopThreadStateTint(thread.state, accent: .green)
    }

    private var threadIconName: String {
        isActive ? "bolt.fill" : "bubble.left"
    }

    private var rowBackground: Color {
        isSelected ? Color.accentColor.opacity(0.14) : Color(nsColor: .controlBackgroundColor).opacity(0.14)
    }

    private var rowStroke: Color {
        isSelected ? Color.accentColor.opacity(0.22) : Color.primary.opacity(0.04)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            headerRow

            HStack(spacing: 10) {
                Label("\(thread.participantCount)", systemImage: "person.2")
                Label("Seq \(thread.lastThreadSeq)", systemImage: "arrow.up.message")
            }
            .font(.caption)
            .foregroundStyle(.secondary)

            Text(secondaryText)
                .font(preview == nil ? .caption.monospaced() : .caption)
                .foregroundStyle(.secondary)
                .lineLimit(2)
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(rowBackground)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .stroke(rowStroke, lineWidth: 1)
        )
        .onHover { hovering in
            isHovering = hovering
        }
        .contextMenu {
            Button {
                onOpenInNewWindow()
            } label: {
                Label("Open in New Window", systemImage: "macwindow")
            }

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

    private var headerRow: some View {
        HStack(alignment: .center, spacing: 8) {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(isSelected ? Color.accentColor.opacity(0.16) : Color.secondary.opacity(0.08))
                .frame(width: 28, height: 28)
                .overlay {
                    Image(systemName: threadIconName)
                        .font(.caption)
                        .foregroundStyle(isActive ? Color.green : .secondary)
                }

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.body.weight(.semibold))
                    .lineLimit(1)
                subtitleRow
            }

            Spacer()

            if isPinned {
                Image(systemName: "pin.fill")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }

            if isHovering || isSelected {
                quickActions
            }

            if desktopThreadShowsActivity(thread.state) {
                Image(systemName: "waveform")
                    .font(.caption)
                    .foregroundStyle(stateTint)
            }

            DesktopStatusPill(label: thread.state.capitalized, tint: isActive ? .green : stateTint)
        }
    }

    private var subtitleRow: some View {
        HStack(spacing: 6) {
            Text(relativeDateText)
                .font(.caption2)
                .foregroundStyle(.secondary)
                .lineLimit(1)

            if isUnread {
                Text("New")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(Color.accentColor)
            }
        }
    }

    private var quickActions: some View {
        HStack(spacing: 6) {
            Button {
                onPin()
            } label: {
                Image(systemName: isPinned ? "pin.slash" : "pin")
            }
            .buttonStyle(.borderless)
            .help(isPinned ? "Unpin thread" : "Pin thread")

            Button {
                onOpenInNewWindow()
            } label: {
                Image(systemName: "macwindow")
            }
            .buttonStyle(.borderless)
            .help("Open in new window")

            Button {
                showThreadDetail = true
            } label: {
                Image(systemName: "info.circle")
            }
            .buttonStyle(.borderless)
            .help("Thread Info")
            .popover(isPresented: $showThreadDetail, arrowEdge: .trailing) {
                ThreadDetailPopover(
                    thread: thread,
                    snapshot: snapshot,
                    participants: participants,
                    onAddAgents: onShowAddAgents,
                    onOpenInNewWindow: onOpenInNewWindow
                )
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
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
    @State private var composerAssistantSelectionIndex = 0

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

    private var workspaceTint: Color {
        if let tintName = agentParticipants.first?.tintName {
            return desktopTintColor(named: tintName)
        }
        if let tintName = timeline.last?.tintName {
            return desktopTintColor(named: tintName)
        }
        return .accentColor
    }

    private var promptSuggestions: [String] {
        if agentParticipants.count > 1 {
            return [
                "@all compare two approaches and disagree where needed",
                "/plan outline the next three implementation steps",
                "review the latest thread state and identify blockers"
            ]
        }

        return [
            "/plan break this task into concrete steps",
            "review the latest activity and propose the next change",
            "summarize what changed in this thread so far"
        ]
    }

    private var mentionSuggestions: [DaemonThreadParticipant] {
        guard let snapshot,
              let mentionContext = desktopActiveComposerMentionContext(in: composerText)
        else {
            return []
        }

        return snapshot.participants.filter(\.isAgent)
            .filter { $0.matchesMentionQuery(mentionContext.query) }
            .sorted { lhs, rhs in
                let lhsHandle = lhs.mentionHandle.localizedCaseInsensitiveCompare(rhs.mentionHandle)
                if lhsHandle != .orderedSame {
                    return lhsHandle == .orderedAscending
                }
                return lhs.displayName.localizedCaseInsensitiveCompare(rhs.displayName) == .orderedAscending
            }
    }

    private var slashCommands: [DesktopSlashCommand] {
        let commands: [DesktopSlashCommand] = [
            .init(command: "plan", template: "/plan ", title: "Plan", description: "Outline the next implementation steps"),
            .init(command: "review", template: "/review ", title: "Review", description: "Inspect the current thread for risks or issues"),
            .init(command: "summarize", template: "/summarize ", title: "Summarize", description: "Condense the current thread state"),
            .init(command: "handoff", template: "/handoff ", title: "Handoff", description: "Prepare a concise handoff for another agent")
        ]

        guard let slashContext = desktopActiveSlashContext(in: composerText) else {
            return []
        }

        if slashContext.query.isEmpty {
            return commands
        }

        return commands.filter {
            $0.command.localizedCaseInsensitiveContains(slashContext.query)
                || $0.title.localizedCaseInsensitiveContains(slashContext.query)
        }
    }

    private var showsComposerAssistant: Bool {
        !mentionSuggestions.isEmpty || desktopActiveComposerMentionContext(in: composerText) != nil || !slashCommands.isEmpty
    }

    private var composerAssistantItems: [DesktopComposerAssistantItem] {
        if desktopActiveComposerMentionContext(in: composerText) != nil {
            return mentionSuggestions.map(DesktopComposerAssistantItem.mention)
        }

        return slashCommands.map(DesktopComposerAssistantItem.slash)
    }

    private var composerAssistantReservedHeight: CGFloat {
        guard showsComposerAssistant else { return 0 }
        let visibleItems = min(composerAssistantItems.count, 4)
        let headerHeight: CGFloat = 34
        let rowHeight = CGFloat(max(visibleItems, 1)) * 42
        return min(220, headerHeight + rowHeight + 20)
    }

    var body: some View {
        VStack(spacing: 16) {
            DesktopHeroSurface(tint: workspaceTint) {
                VStack(alignment: .leading, spacing: 18) {
                    HStack(alignment: .top, spacing: 18) {
                        VStack(alignment: .leading, spacing: 10) {
                            Text(titleText)
                                .font(.system(size: 28, weight: .semibold, design: .rounded))
                                .lineLimit(2)
                            Text(thread.workingDir)
                                .font(.caption.monospaced())
                                .foregroundStyle(.secondary)
                                .textSelection(.enabled)

                            HStack(spacing: 8) {
                                DesktopStatusPill(label: thread.state.capitalized, tint: .secondary)
                                DesktopStatusPill(label: "\(thread.participantCount) participants", tint: workspaceTint)
                                DesktopStatusPill(label: "Seq \(thread.lastThreadSeq)", tint: .secondary)
                            }
                        }

                        Spacer(minLength: 0)

                        VStack(alignment: .trailing, spacing: 8) {
                            if isLoadingThreadContent {
                                ProgressView()
                                    .controlSize(.small)
                            } else if connectionState.isOnline {
                                DesktopStatusPill(label: "Live", tint: .green)
                            }

                            Text("Attached workspace")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }

                    if let snapshot {
                        ParticipantPicker(participants: snapshot.participants, selectedParticipantIDs: $selectedParticipantIDs)
                    }
                }
            }

            DesktopSurface(padding: 0) {
                VStack(alignment: .leading, spacing: 0) {
                    HStack {
                        Text("Activity")
                            .font(.headline)
                        Spacer()
                        Text("\(timeline.count) events")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .padding(.horizontal, 18)
                    .padding(.top, 16)
                    .padding(.bottom, 12)

                    Divider()

                    Group {
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
                    .padding(18)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)

            DesktopSurface {
                VStack(alignment: .leading, spacing: 14) {
                    HStack(alignment: .firstTextBaseline) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text("Composer")
                                .font(.headline)
                            Text("Use @mentions to direct work or keep all targets selected to broadcast.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        if !agentParticipants.isEmpty {
                            DesktopStatusPill(
                                label: "\(selectedParticipantIDs.count) of \(agentParticipants.count) targets",
                                tint: selectedParticipantIDs.count == agentParticipants.count ? .green : .orange
                            )
                        }
                    }

                    ZStack(alignment: .topLeading) {
                        TextEditor(text: $composerText)
                            .font(.body)
                            .focused($isComposerFocused)
                            .frame(minHeight: 132)
                            .padding(12)
                            .background(
                                RoundedRectangle(cornerRadius: 18, style: .continuous)
                                    .fill(Color.black.opacity(0.035))
                            )
                            .overlay(
                                RoundedRectangle(cornerRadius: 18, style: .continuous)
                                    .stroke(
                                        isComposerFocused ? Color.accentColor.opacity(0.55) : Color.primary.opacity(0.06),
                                        lineWidth: isComposerFocused ? 1.5 : 1
                                    )
                            )

                        if composerText.isEmpty {
                            VStack(alignment: .leading, spacing: 6) {
                                Text("Message the selected agents...")
                                    .foregroundStyle(.tertiary)
                                Text("Try `@codex compare two implementation paths` or `/plan outline the next steps`")
                                    .font(.caption)
                                    .foregroundStyle(.tertiary)
                            }
                            .padding(.horizontal, 18)
                            .padding(.vertical, 20)
                            .allowsHitTesting(false)
                        }
                    }
                    .overlay(alignment: .bottomLeading) {
                        if showsComposerAssistant {
                            DesktopSurface(padding: 14) {
                                VStack(alignment: .leading, spacing: 10) {
                                    if desktopActiveComposerMentionContext(in: composerText) != nil {
                                        HStack {
                                            Text("Mention agent")
                                                .font(.caption.weight(.semibold))
                                                .foregroundStyle(.secondary)
                                            Spacer()
                                            Text("In this thread")
                                                .font(.caption2.weight(.medium))
                                                .foregroundStyle(.secondary)
                                        }

                                        if mentionSuggestions.isEmpty {
                                            Text("No agents match this mention yet.")
                                                .font(.footnote)
                                                .foregroundStyle(.secondary)
                                        } else {
                                            ForEach(Array(composerAssistantItems.enumerated()), id: \.element.id) { index, item in
                                                composerAssistantRow(item, index: index)
                                            }
                                        }
                                    } else if !slashCommands.isEmpty {
                                        HStack {
                                            Text("Command")
                                                .font(.caption.weight(.semibold))
                                                .foregroundStyle(.secondary)
                                            Spacer()
                                            Text("Templates")
                                                .font(.caption2.weight(.medium))
                                                .foregroundStyle(.secondary)
                                        }

                                        ForEach(Array(composerAssistantItems.enumerated()), id: \.element.id) { index, item in
                                            composerAssistantRow(item, index: index)
                                        }
                                    }
                                }
                            }
                            .frame(maxWidth: 480)
                            .padding(.leading, 8)
                            .offset(y: composerAssistantReservedHeight > 0 ? composerAssistantReservedHeight - 12 : 0)
                        }
                    }
                    .padding(.bottom, composerAssistantReservedHeight)

                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(promptSuggestions, id: \.self) { suggestion in
                                Button {
                                    if composerText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                                        composerText = suggestion
                                    } else {
                                        composerText += composerText.hasSuffix("\n") ? suggestion : "\n" + suggestion
                                    }
                                    isComposerFocused = true
                                } label: {
                                    Text(suggestion)
                                        .font(.caption)
                                        .lineLimit(1)
                                        .padding(.horizontal, 12)
                                        .padding(.vertical, 8)
                                        .background(Color.black.opacity(0.035), in: Capsule())
                                        .overlay(
                                            Capsule()
                                                .stroke(Color.primary.opacity(0.05), lineWidth: 1)
                                        )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }

                    HStack(alignment: .center, spacing: 12) {
                        Label(composerHint, systemImage: "command")
                            .font(.caption)
                            .foregroundStyle(selectedParticipantIDs.isEmpty && !agentParticipants.isEmpty ? Color.orange : .secondary)

                        Spacer()

                        if !composerText.isEmpty {
                            Button("Clear") {
                                composerText = ""
                            }
                            .buttonStyle(.bordered)
                        }

                        Button("Send", action: onSend)
                            .keyboardShortcut(.return, modifiers: [.command])
                            .buttonStyle(.borderedProminent)
                            .disabled(!canSend)
                    }
                }
            }
        }
        .padding(24)
        .onChange(of: composerText) { _, _ in
            composerAssistantSelectionIndex = 0
        }
        .onKeyPress(.downArrow) {
            guard !composerAssistantItems.isEmpty else { return .ignored }
            composerAssistantSelectionIndex = min(composerAssistantSelectionIndex + 1, composerAssistantItems.count - 1)
            return .handled
        }
        .onKeyPress(.upArrow) {
            guard !composerAssistantItems.isEmpty else { return .ignored }
            composerAssistantSelectionIndex = max(composerAssistantSelectionIndex - 1, 0)
            return .handled
        }
        .onKeyPress(.return) {
            guard !composerAssistantItems.isEmpty else { return .ignored }
            applyComposerAssistantSelection()
            return .handled
        }
    }

    private func applyMentionSuggestion(_ participant: DaemonThreadParticipant) {
        guard let mentionContext = desktopActiveComposerMentionContext(in: composerText) else { return }
        composerText.replaceSubrange(
            mentionContext.replacementRange,
            with: "@\(participant.mentionHandle) "
        )
        isComposerFocused = true
    }

    private func applySlashCommand(_ command: DesktopSlashCommand) {
        guard let slashContext = desktopActiveSlashContext(in: composerText) else { return }
        composerText.replaceSubrange(
            slashContext.replacementRange,
            with: command.template
        )
        isComposerFocused = true
    }

    @ViewBuilder
    private func composerAssistantRow(_ item: DesktopComposerAssistantItem, index: Int) -> some View {
        Button {
            activateComposerAssistant(item)
        } label: {
            HStack(spacing: 10) {
                switch item {
                case .mention(let participant):
                    Image(systemName: participant.family.symbolName)
                        .foregroundStyle(desktopTintColor(named: participant.tintName))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(participant.displayName)
                            .font(.callout.weight(.medium))
                        Text("@\(participant.mentionHandle)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                case .slash(let command):
                    Text("/\(command.command)")
                        .font(.caption.monospaced())
                        .foregroundStyle(workspaceTint)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(workspaceTint.opacity(0.10), in: Capsule())
                    VStack(alignment: .leading, spacing: 2) {
                        Text(command.title)
                            .font(.callout.weight(.medium))
                        Text(command.description)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                Spacer()
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(index == clampedComposerAssistantSelectionIndex ? workspaceTint.opacity(0.10) : Color.clear)
            )
        }
        .buttonStyle(.plain)
    }

    private var clampedComposerAssistantSelectionIndex: Int {
        guard !composerAssistantItems.isEmpty else { return 0 }
        return min(composerAssistantSelectionIndex, composerAssistantItems.count - 1)
    }

    private func applyComposerAssistantSelection() {
        guard !composerAssistantItems.isEmpty else { return }
        activateComposerAssistant(composerAssistantItems[clampedComposerAssistantSelectionIndex])
    }

    private func activateComposerAssistant(_ item: DesktopComposerAssistantItem) {
        switch item {
        case .mention(let participant):
            applyMentionSuggestion(participant)
        case .slash(let command):
            applySlashCommand(command)
        }
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
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .center) {
                HStack(spacing: 10) {
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .fill(desktopTintColor(named: entry.tintName).opacity(0.14))
                        .frame(width: 30, height: 30)
                        .overlay {
                            Image(systemName: symbolName)
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(desktopTintColor(named: entry.tintName))
                        }

                    VStack(alignment: .leading, spacing: 2) {
                        Text(entry.title)
                            .font(.headline)
                        if let status = entry.status, !status.isEmpty {
                            Text(status.capitalized)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }

                Spacer()
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
                VStack(alignment: .leading, spacing: 8) {
                    Text("Tools")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    ForEach(entry.orderedToolActivities) { activity in
                        HStack(spacing: 10) {
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
                        .padding(10)
                        .background(Color.black.opacity(0.025), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                    }
                }
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .fill(Color(nsColor: .controlBackgroundColor).opacity(0.8))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .stroke(desktopTintColor(named: entry.tintName).opacity(0.12), lineWidth: 1)
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
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Targets")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Text(selectedParticipantIDs.count == allAgentIDs.count ? "Broadcasting to all selected agents" : "Selective routing is enabled")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }

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
                            HStack(spacing: 8) {
                                Image(systemName: participant.family.symbolName)
                                Text(participant.displayName)
                                    .lineLimit(1)
                            }
                            .font(.callout.weight(.medium))
                            .padding(.horizontal, 12)
                            .padding(.vertical, 10)
                            .background(
                                Capsule()
                                    .fill(
                                        isSelected
                                            ? desktopTintColor(named: participant.tintName).opacity(0.18)
                                            : Color.secondary.opacity(0.08)
                                    )
                            )
                            .overlay(
                                Capsule()
                                    .stroke(
                                        isSelected
                                            ? desktopTintColor(named: participant.tintName).opacity(0.28)
                                            : Color.primary.opacity(0.05),
                                        lineWidth: 1
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

struct ThreadDetailPopover: View {
    let thread: DaemonThreadSummary
    let snapshot: DaemonThreadSnapshot?
    let participants: [DaemonThreadParticipant]
    let onAddAgents: () -> Void
    let onOpenInNewWindow: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.headline)
                    Text(thread.workingDir)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button {
                    onAddAgents()
                } label: {
                    Text("Add")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }

            Divider()

            InspectorSection("Thread") {
                Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 6) {
                    GridRow {
                        InspectorFactRow("Created", value: thread.createdAtDate.formatted(date: .abbreviated, time: .shortened))
                        InspectorFactRow("State", value: thread.state.capitalized)
                    }
                    GridRow {
                        InspectorFactRow("Participants", value: "\(participants.count)")
                        InspectorFactRow("Last Seq", value: "\(snapshot?.lastThreadSeq ?? thread.lastThreadSeq)")
                    }
                }
            }

            Divider()

            InspectorSection("Participants") {
                if participants.isEmpty {
                    Text("No participants")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(participants) { participant in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Image(systemName: participant.family.symbolName)
                                    .foregroundStyle(desktopTintColor(named: participant.tintName))
                                Text(participant.displayName)
                                    .font(.body.weight(.medium))
                                Text(participant.kindTitle)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                Spacer()
                                Text(participant.state.capitalized)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Text("@\(participant.mentionHandle)")
                                .font(.caption.monospaced())
                                .foregroundStyle(.secondary)
                        }
                        .padding(.vertical, 4)

                        if participant.id != participants.last?.id {
                            Divider()
                        }
                    }
                }
            }

            if let snapshot {
                Divider()

                InspectorSection("Snapshot") {
                    Text(snapshot.title ?? "Untitled")
                        .font(.body.weight(.medium))
                }
            }

            Button {
                onOpenInNewWindow()
            } label: {
                Label("Open in New Window", systemImage: "macwindow")
            }
            .buttonStyle(.bordered)
        }
        .padding(16)
        .frame(width: 320)
    }

    private var title: String {
        thread.threadID.prefix(8).map { String($0) }.joined() + "..."
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
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Inspector")
                            .font(.headline)
                        Text(thread == nil ? "Open a thread to inspect live details." : "Utility details for the current thread.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Add Agents") {
                        showAddAgentsSheet = true
                    }
                    .disabled(thread == nil)
                }
                .padding(.bottom, 14)

                Divider()

                if let thread {
                    InspectorSection("Thread") {
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
                    }

                    Divider()

                    if !selectedParticipants.isEmpty {
                        InspectorSection("Current Targets") {
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

                        Divider()
                    }
                } else {
                    InspectorSection("Thread") {
                        Text("No thread selected")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    }

                    Divider()
                }

                InspectorSection("Participants") {
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
                            .padding(.vertical, 4)

                            if participant.id != participants.last?.id {
                                Divider()
                            }
                        }
                    }
                }

                if let snapshot {
                    Divider()

                    InspectorSection("Snapshot") {
                        VStack(alignment: .leading, spacing: 8) {
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

struct DesktopVerticalTabRail: View {
    @Binding var selectedTab: String

    var body: some View {
        VStack(spacing: 0) {
            Spacer().frame(height: 12)

            VStack(spacing: 6) {
                DesktopTabRailButton(
                    icon: "bubble.left.and.bubble.right",
                    label: "Threads",
                    tab: "chats",
                    selectedTab: $selectedTab
                )
                DesktopTabRailButton(
                    icon: "person.2",
                    label: "Agents",
                    tab: "agents",
                    selectedTab: $selectedTab
                )
                DesktopTabRailButton(
                    icon: "gearshape",
                    label: "Settings",
                    tab: "settings",
                    selectedTab: $selectedTab
                )
            }

            Spacer()
        }
        .frame(width: 52)
        .background(Color(nsColor: .windowBackgroundColor).opacity(0.45))
    }
}

struct DesktopTabRailButton: View {
    let icon: String
    let label: String
    let tab: String
    @Binding var selectedTab: String

    private var isSelected: Bool { selectedTab == tab }

    var body: some View {
        Button {
            selectedTab = tab
        } label: {
            VStack(spacing: 3) {
                Image(systemName: isSelected ? filledIcon : icon)
                    .font(.system(size: 17))
                    .frame(width: 36, height: 36)
                    .background(
                        isSelected
                            ? RoundedRectangle(cornerRadius: 8).fill(Color.accentColor.opacity(0.15))
                            : nil
                    )
            }
        }
        .buttonStyle(.plain)
        .foregroundStyle(isSelected ? .primary : .secondary)
        .help(label)
    }

    private var filledIcon: String {
        switch icon {
        case "bubble.left.and.bubble.right": return "bubble.left.and.bubble.right.fill"
        case "person.2": return "person.2.fill"
        case "gearshape": return "gearshape.fill"
        default: return icon
        }
    }
}

struct DesktopSettingsSidebarPanel: View {
    @EnvironmentObject private var store: DaemonChatStore

    @State private var daemonURLDraft = ""

    private var presentation: AgentChatDesktopConnectionPresentation {
        AgentChatDesktopConnectionPresentation(state: store.connectionState)
    }

    var body: some View {
        ScrollView {
            Form {
                Section("Connection") {
                    TextField(
                        "ws://127.0.0.1:9390 or agentchat://connect?...",
                        text: $daemonURLDraft,
                        axis: .vertical
                    )
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(3...6)

                    HStack(spacing: 10) {
                        Button("Apply and Connect") {
                            store.updateDaemonURL(daemonURLDraft)
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(daemonURLDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

                        Button("Reconnect") {
                            store.reconnectNow()
                        }
                        .disabled(!store.hasConfiguredDaemonURL)

                        Button("Disconnect") {
                            store.disconnect()
                        }
                        .disabled(!store.hasConfiguredDaemonURL)
                    }

                    HStack(spacing: 10) {
                        Image(systemName: presentation.systemImage)
                            .foregroundStyle(desktopTintColor(named: presentation.tintName))
                        VStack(alignment: .leading, spacing: 2) {
                            Text(presentation.title)
                                .font(.body.weight(.medium))
                            Text(presentation.subtitle)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }

                    if let errorSummary = store.desktopConnectionErrorSummary {
                        Text(errorSummary)
                            .font(.callout)
                            .foregroundStyle(.orange)
                    }
                }

                Section("Supported Links") {
                    Text("Paste a direct ws:// or wss:// daemon URL, or an agentchat://connect link for direct or relay pairing.")
                        .foregroundStyle(.secondary)
                }
            }
            .formStyle(.grouped)
        }
        .onAppear {
            daemonURLDraft = store.daemonURL
        }
        .onChange(of: store.daemonURL) { _, newValue in
            daemonURLDraft = newValue
        }
    }
}
