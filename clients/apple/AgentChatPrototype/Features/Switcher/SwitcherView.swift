import SwiftUI

struct SwitcherView: View {
    @EnvironmentObject private var store: DemoStore
    @Binding var selectedIssueID: UUID?

    @State private var mode: SwitcherMode = .grid

    private var cards: [WorkspaceCardModel] {
        store.workspaceCards
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                toolbar
                summaryStrip
                content
            }
            .padding(AppSpacing.lg)
        }
        .navigationTitle("Switcher")
    }

    private var toolbar: some View {
        CardSurface(accent: .blue) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Parallel workspaces")
                        .font(.title2.weight(.semibold))
                    Text("Keep multiple tasks, sessions, and agent runs visible at once.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Picker("Mode", selection: $mode) {
                    ForEach(SwitcherMode.allCases) { mode in
                        Text(mode.title).tag(mode)
                    }
                }
                .pickerStyle(.segmented)
                .frame(maxWidth: 280)
            }
        }
    }

    private var summaryStrip: some View {
        CardSurface(accent: .purple) {
            HStack(spacing: AppSpacing.xl) {
                MetricLabel(title: "Visible Workspaces", value: "\(cards.count)")
                MetricLabel(title: "Running", value: "\(cards.filter { $0.state == .running }.count)")
                MetricLabel(title: "Waiting", value: "\(cards.filter { $0.state == .waitingInput }.count)")
                MetricLabel(title: "Completed", value: "\(cards.filter { $0.state == .completed }.count)")
                Spacer()
            }
        }
    }

    @ViewBuilder
    private var content: some View {
        switch mode {
        case .list:
            VStack(spacing: AppSpacing.md) {
                ForEach(cards) { card in
                    Button {
                        selectedIssueID = card.issueID
                    } label: {
                        WorkspaceRow(card: card, isSelected: selectedIssueID == card.issueID)
                    }
                    .buttonStyle(.plain)
                }
            }
        case .grid:
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 280), spacing: AppSpacing.md)], spacing: AppSpacing.md) {
                ForEach(cards) { card in
                    Button {
                        selectedIssueID = card.issueID
                    } label: {
                        WorkspaceCard(card: card, isSelected: selectedIssueID == card.issueID)
                    }
                    .buttonStyle(.plain)
                }
            }
        case .focus:
            HStack(alignment: .top, spacing: AppSpacing.md) {
                if let selected = cards.first(where: { $0.issueID == selectedIssueID }) ?? cards.first {
                    WorkspaceFocusCard(card: selected)
                        .frame(maxWidth: .infinity, alignment: .topLeading)
                }

                VStack(spacing: AppSpacing.md) {
                    ForEach(cards.filter { $0.issueID != selectedIssueID }) { card in
                        Button {
                            selectedIssueID = card.issueID
                        } label: {
                            WorkspaceCard(card: card, isSelected: false)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .frame(width: 320)
            }
        }
    }
}

private struct WorkspaceCard: View {
    let card: WorkspaceCardModel
    let isSelected: Bool

    var body: some View {
        CardSurface(accent: card.state.badgeColor, isSelected: isSelected) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("#\(card.issueNumber)")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                        Text(card.title)
                            .font(.headline)
                            .multilineTextAlignment(.leading)
                    }
                    Spacer()
                    StatusBadge(text: card.state.title, color: card.state.badgeColor)
                }

                HStack(spacing: 8) {
                    ForEach(card.participants, id: \.self) { participant in
                        PillView(text: participant, color: accent(for: participant))
                    }
                }

                Text(card.latestPreview)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.leading)

                HStack {
                    MetricLabel(title: "Elapsed", value: AppFormatters.durationString(seconds: card.elapsedSeconds))
                    Spacer()
                    if let activeTool = card.activeTool {
                        PillView(text: activeTool, color: .orange)
                    }
                }
            }
        }
    }

    private func accent(for participant: String) -> ColorToken {
        switch participant {
        case "Claude": return .blue
        case "Codex": return .green
        case "Pi": return .purple
        default: return .gray
        }
    }
}

private struct WorkspaceRow: View {
    let card: WorkspaceCardModel
    let isSelected: Bool

    var body: some View {
        CardSurface(accent: card.state.badgeColor, isSelected: isSelected) {
            HStack(spacing: AppSpacing.md) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("#\(card.issueNumber) · \(card.title)")
                        .font(.headline)
                    Text(card.latestPreview)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                HStack(spacing: AppSpacing.md) {
                    ForEach(card.participants, id: \.self) { participant in
                        PillView(text: participant, color: .gray)
                    }
                    StatusBadge(text: card.state.title, color: card.state.badgeColor)
                    Text(AppFormatters.durationString(seconds: card.elapsedSeconds))
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.secondary)
                }
            }
        }
    }
}

private struct WorkspaceFocusCard: View {
    let card: WorkspaceCardModel

    var body: some View {
        CardSurface(accent: card.state.badgeColor, isSelected: true) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                HStack {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Focused workspace")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                        Text("#\(card.issueNumber) \(card.title)")
                            .font(.title2.weight(.bold))
                    }
                    Spacer()
                    StatusBadge(text: card.state.title, color: card.state.badgeColor)
                }

                HStack(spacing: 8) {
                    ForEach(card.participants, id: \.self) { participant in
                        PillView(text: participant, color: .gray)
                    }
                }

                CardSurface(accent: .gray) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Latest output")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                        Text(card.latestPreview)
                            .font(.body)
                    }
                }

                HStack(spacing: AppSpacing.xl) {
                    MetricLabel(title: "Elapsed", value: AppFormatters.durationString(seconds: card.elapsedSeconds))
                    MetricLabel(title: "Tool", value: card.activeTool ?? "None")
                    Spacer()
                }
            }
        }
    }
}
