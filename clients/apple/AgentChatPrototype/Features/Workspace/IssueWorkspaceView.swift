import SwiftUI

struct IssueWorkspaceView: View {
    @EnvironmentObject private var store: DemoStore
    let issueID: UUID

    @State private var composerText = ""
    @State private var selectedTargets: Set<String> = []

    var body: some View {
        if let issue = store.issue(for: issueID) {
            VStack(spacing: 0) {
                IssueWorkspaceHeader(issue: issue)
                    .padding(.horizontal, AppSpacing.lg)
                    .padding(.top, AppSpacing.lg)

                HStack(alignment: .top, spacing: AppSpacing.md) {
                    ChatTimelineColumn(items: store.timeline(for: issueID))
                        .frame(maxWidth: .infinity, maxHeight: .infinity)

                    WorkspaceSidePanel(issueID: issueID)
                        .frame(width: 320)
                }
                .padding(.horizontal, AppSpacing.lg)
                .padding(.vertical, AppSpacing.md)
                .frame(maxWidth: .infinity, maxHeight: .infinity)

                MessageComposerBar(
                    issue: issue,
                    text: $composerText,
                    selectedTargets: $selectedTargets,
                    onSend: {
                        store.sendMessage(
                            issueID: issueID,
                            text: composerText,
                            targets: Array(selectedTargets)
                        )
                        composerText = ""
                    }
                )
                .padding(.horizontal, AppSpacing.lg)
                .padding(.bottom, AppSpacing.lg)
            }
            .background(Color.appCanvasBackground)
            .navigationTitle(issue.title)
            .onAppear {
                seedTargets(from: issue)
            }
        } else {
            EmptyStateView(
                title: "Issue not found",
                message: "The selected issue is no longer available in the mock store.",
                systemImage: "exclamationmark.triangle"
            )
        }
    }

    private func seedTargets(from issue: Issue) {
        if selectedTargets.isEmpty {
            selectedTargets = Set(issue.agentNames)
        }
    }
}

private struct IssueWorkspaceHeader: View {
    let issue: Issue

    var body: some View {
        CardSurface(accent: issue.status.badgeColor) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                HStack(alignment: .top, spacing: AppSpacing.md) {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 8) {
                            Text("#\(issue.number)")
                                .font(.headline.monospacedDigit())
                                .foregroundStyle(.secondary)
                            StatusBadge(text: issue.status.title, color: issue.status.badgeColor)
                        }

                        Text(issue.title)
                            .font(.title2.weight(.bold))

                        Text(issue.summary)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }

                    Spacer()

                    HStack(spacing: 8) {
                        Button("Start") {}
                            .buttonStyle(.borderedProminent)
                        Button("Distill") {}
                            .buttonStyle(.bordered)
                        Button("Switcher") {}
                            .buttonStyle(.bordered)
                    }
                }

                HStack(spacing: AppSpacing.md) {
                    ForEach(issue.assignees) { participant in
                        HStack(spacing: 8) {
                            AvatarView(title: participant.displayName, accent: participant.accent, size: 28)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(participant.displayName)
                                    .font(.subheadline.weight(.semibold))
                                Text("Active participant")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                        .padding(.vertical, 6)
                        .padding(.horizontal, 10)
                        .background(participant.accent.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    }
                }
            }
        }
    }
}

private struct ChatTimelineColumn: View {
    let items: [TimelineItem]

    var body: some View {
        CardSurface(accent: .blue) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: AppSpacing.md) {
                        ForEach(items) { item in
                            TimelineItemView(item: item)
                                .id(item.id)
                        }
                    }
                    .padding(.vertical, 4)
                }
                .onChange(of: items.count) { _, _ in
                    guard let lastID = items.last?.id else { return }
                    withAnimation(.easeOut(duration: 0.2)) {
                        proxy.scrollTo(lastID, anchor: .bottom)
                    }
                }
            }
        }
    }
}

private struct TimelineItemView: View {
    let item: TimelineItem

    var body: some View {
        switch item.payload {
        case .system(let event):
            SystemEventBubble(text: event.text, timestamp: item.timestamp)
        case .userMessage(let message):
            HStack {
                Spacer(minLength: 60)
                UserMessageBubble(message: message, timestamp: item.timestamp)
            }
        case .agentMessage(let message):
            HStack(alignment: .top) {
                AgentMessageBubble(message: message, timestamp: item.timestamp)
                Spacer(minLength: 60)
            }
        case .thinking(let event):
            ThinkingBubble(event: event, timestamp: item.timestamp)
        case .toolCall(let event):
            ToolCallCard(event: event, timestamp: item.timestamp)
        case .plan(let event):
            PlanCard(event: event, timestamp: item.timestamp)
        case .turnEnd(let event):
            TurnEndMarker(event: event, timestamp: item.timestamp)
        }
    }
}

private struct UserMessageBubble: View {
    let message: ChatMessage
    let timestamp: Date

    var body: some View {
        VStack(alignment: .trailing, spacing: 6) {
            Text("You")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            Text(message.text)
                .font(.body)
                .multilineTextAlignment(.leading)
                .padding(.horizontal, 14)
                .padding(.vertical, 12)
                .background(Color.blue, in: RoundedRectangle(cornerRadius: AppRadius.bubble, style: .continuous))
                .foregroundStyle(.white)
            Text(AppFormatters.timeString(from: timestamp))
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }
}

private struct AgentMessageBubble: View {
    let message: ChatMessage
    let timestamp: Date

    var body: some View {
        let accent: ColorToken = {
            switch message.senderRole {
            case .human:
                return .gray
            case .agent(let kind):
                switch kind {
                case .claude: return .blue
                case .codex: return .green
                case .pi: return .purple
                case .opencode: return .orange
                case .human: return .gray
                }
            }
        }()

        return HStack(alignment: .top, spacing: 10) {
            AvatarView(title: message.senderName, accent: accent)
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 8) {
                    Text(message.senderName)
                        .font(.caption.weight(.semibold))
                    if message.isStreaming {
                        StatusBadge(text: "Streaming", color: accent)
                    }
                }
                Text(message.text.isEmpty ? "…" : message.text)
                    .font(.body)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 12)
                    .background(accent.color.opacity(0.12), in: RoundedRectangle(cornerRadius: AppRadius.bubble, style: .continuous))
                Text(AppFormatters.timeString(from: timestamp))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

private struct ThinkingBubble: View {
    let event: ThinkingEvent
    let timestamp: Date

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "brain.head.profile")
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 6) {
                Text("\(event.agentName) is thinking")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                Text(event.text)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Text(AppFormatters.timeString(from: timestamp))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: AppRadius.bubble, style: .continuous))
    }
}

private struct ToolCallCard: View {
    let event: ToolCallEvent
    let timestamp: Date

    var body: some View {
        CardSurface(accent: event.status.badgeColor) {
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(event.agentName)
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                        Text(event.title)
                            .font(.headline)
                    }
                    Spacer()
                    StatusBadge(text: event.status.title, color: event.status.badgeColor)
                }

                if let contentPreview = event.contentPreview {
                    Text(contentPreview)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }

                HStack {
                    PillView(text: event.toolName, color: .gray)
                    Spacer()
                    Text(AppFormatters.timeString(from: timestamp))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }
}

private struct PlanCard: View {
    let event: PlanEvent
    let timestamp: Date

    var body: some View {
        CardSurface(accent: .purple) {
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(event.agentName)
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                        Text(event.title)
                            .font(.headline)
                    }
                    Spacer()
                    Text(AppFormatters.timeString(from: timestamp))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }

                ForEach(event.steps, id: \.self) { step in
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: "checklist")
                            .foregroundStyle(.purple)
                        Text(step)
                            .font(.subheadline)
                    }
                }
            }
        }
    }
}

private struct SystemEventBubble: View {
    let text: String
    let timestamp: Date

    var body: some View {
        HStack(spacing: 8) {
            Spacer()
            Text(text)
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
            Text(AppFormatters.timeString(from: timestamp))
                .font(.caption2)
                .foregroundStyle(.secondary)
            Spacer()
        }
        .padding(.vertical, 4)
    }
}

private struct TurnEndMarker: View {
    let event: TurnEndEvent
    let timestamp: Date

    var body: some View {
        HStack(spacing: 10) {
            Rectangle()
                .fill(Color.primary.opacity(0.08))
                .frame(height: 1)
            Text("\(event.agentName) finished · \(event.reason)")
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
            Text(AppFormatters.timeString(from: timestamp))
                .font(.caption2)
                .foregroundStyle(.secondary)
            Rectangle()
                .fill(Color.primary.opacity(0.08))
                .frame(height: 1)
        }
    }
}

private struct WorkspaceSidePanel: View {
    @EnvironmentObject private var store: DemoStore
    let issueID: UUID

    @State private var tab: SidePanelTab = .timeline

    private enum SidePanelTab: String, CaseIterable, Identifiable {
        case timeline
        case sessions
        case panels

        var id: Self { self }

        var title: String {
            switch self {
            case .timeline: return "Timeline"
            case .sessions: return "Sessions"
            case .panels: return "Panels"
            }
        }
    }

    private var groupedSkillSections: [SkillSection] {
        let grouped = Dictionary(grouping: store.skillCards(for: issueID), by: \.scope)

        return grouped
            .map { scope, skills in
                SkillSection(
                    scope: scope,
                    accent: skills.first?.accent ?? .purple,
                    skills: skills.sorted {
                        $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending
                    }
                )
            }
            .sorted { lhs, rhs in
                if lhs.scope.sortRank != rhs.scope.sortRank {
                    return lhs.scope.sortRank < rhs.scope.sortRank
                }
                return lhs.scope.title.localizedCaseInsensitiveCompare(rhs.scope.title) == .orderedAscending
            }
    }

    var body: some View {
        CardSurface(accent: .gray) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                Picker("Panel", selection: $tab) {
                    ForEach(SidePanelTab.allCases) { tab in
                        Text(tab.title).tag(tab)
                    }
                }
                .pickerStyle(.segmented)

                switch tab {
                case .timeline:
                    ScrollView {
                        VStack(alignment: .leading, spacing: AppSpacing.sm) {
                            ForEach(store.timeline(for: issueID).suffix(12)) { item in
                                TimelineEventRow(item: item)
                            }
                        }
                    }
                case .sessions:
                    ScrollView {
                        VStack(spacing: AppSpacing.sm) {
                            ForEach(store.sessions(for: issueID)) { session in
                                SessionMiniCard(session: session)
                            }
                        }
                    }
                case .panels:
                    ScrollView {
                        VStack(spacing: AppSpacing.sm) {
                            ForEach(store.sessions(for: issueID)) { session in
                                CardSurface(accent: session.state.badgeColor) {
                                    VStack(alignment: .leading, spacing: 8) {
                                        Text(session.title)
                                            .font(.headline)
                                        Text(session.activeToolName ?? session.latestEventText)
                                            .font(.subheadline)
                                            .foregroundStyle(.secondary)
                                        StatusBadge(text: session.state.title, color: session.state.badgeColor)
                                    }
                                }
                            }

                            ForEach(groupedSkillSections) { section in
                                SkillSectionCard(section: section)
                            }
                        }
                    }
                }
            }
        }
    }
}

private struct SkillSection: Identifiable {
    let scope: SkillScope
    let accent: ColorToken
    let skills: [SkillCardModel]

    var id: String { scope.id }
}

private struct SkillSectionCard: View {
    let section: SkillSection

    var body: some View {
        CardSurface(accent: section.accent) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: section.scope.systemImage)
                        .font(.headline)
                        .foregroundStyle(section.accent.color)
                        .frame(width: 34, height: 34)
                        .background(section.accent.color.opacity(0.12), in: RoundedRectangle(cornerRadius: 12, style: .continuous))

                    VStack(alignment: .leading, spacing: 4) {
                        Text(section.scope.title)
                            .font(.headline)
                        Text(section.scope.subtitle)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    Spacer(minLength: 0)

                    PillView(
                        text: "\(section.skills.count) files",
                        color: section.accent,
                        isSelected: true
                    )
                }

                VStack(spacing: 8) {
                    ForEach(section.skills) { skill in
                        SkillRowCard(skill: skill)
                    }
                }
            }
        }
    }
}

private struct SkillRowCard: View {
    let skill: SkillCardModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(skill.title)
                        .font(.subheadline.weight(.semibold))
                    Text(skill.path)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
                Spacer(minLength: 0)
                Text("Updated \(AppFormatters.relativeString(from: skill.updatedAt))")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            Text(skill.summary)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(skill.accent.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
}

private struct TimelineEventRow: View {
    let item: TimelineItem

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Text(AppFormatters.timeString(from: item.timestamp))
                .font(.caption2.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 54, alignment: .leading)
            Text(item.payload.summaryText)
                .font(.caption)
                .foregroundStyle(.primary)
            Spacer()
        }
        .padding(.vertical, 4)
    }
}

private struct SessionMiniCard: View {
    let session: WorkspaceSession

    var body: some View {
        CardSurface(accent: session.state.badgeColor) {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text(session.agentName)
                        .font(.headline)
                    Spacer()
                    StatusBadge(text: session.state.title, color: session.state.badgeColor)
                }
                Text(session.latestEventText)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                HStack {
                    PillView(text: AppFormatters.durationString(seconds: session.elapsedSeconds), color: .gray)
                    if let activeToolName = session.activeToolName {
                        PillView(text: activeToolName, color: .orange)
                    }
                }
            }
        }
    }
}

private struct MessageComposerBar: View {
    let issue: Issue
    @Binding var text: String
    @Binding var selectedTargets: Set<String>
    let onSend: () -> Void

    var body: some View {
        CardSurface(accent: .green) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(issue.agentNames, id: \.self) { name in
                            Button {
                                if selectedTargets.contains(name) {
                                    selectedTargets.remove(name)
                                } else {
                                    selectedTargets.insert(name)
                                }
                            } label: {
                                PillView(
                                    text: "@\(name)",
                                    color: color(for: name),
                                    isSelected: selectedTargets.contains(name)
                                )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }

                TextEditor(text: $text)
                    .frame(minHeight: 84)
                    .padding(8)
                    .background(Color.appInputBackground, in: RoundedRectangle(cornerRadius: 14, style: .continuous))

                HStack(spacing: 8) {
                    quickAction("Review this implementation")
                    quickAction("Summarize the risky parts")
                    quickAction("Plan the next step")
                    Spacer()
                    Button("Send", action: onSend)
                        .buttonStyle(.borderedProminent)
                        .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
    }

    private func quickAction(_ title: String) -> some View {
        Button(title) {
            text = title
        }
        .buttonStyle(.bordered)
    }

    private func color(for name: String) -> ColorToken {
        switch name {
        case "Claude": return .blue
        case "Codex": return .green
        case "Pi": return .purple
        default: return .gray
        }
    }
}
