import SwiftUI

struct AgentChatDesktopSettingsView: View {
    @EnvironmentObject private var workspaceStore: WorkspaceStore
    @EnvironmentObject private var env: DesktopEnvironment

    @State private var daemonURLDraft = ""

    private var presentation: AgentChatDesktopConnectionPresentation {
        AgentChatDesktopConnectionPresentation(state: env.connectionState)
    }

    var body: some View {
        TabView {
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
                            applyDaemonURL()
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(daemonURLDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

                        Button("Reconnect") {
                            env.reconnectNow()
                        }
                        .disabled(!env.hasConfiguredDaemonURL)

                        Button("Disconnect") {
                            env.disconnect()
                        }
                        .disabled(!env.hasConfiguredDaemonURL)
                    }

                    HStack(spacing: 10) {
                        Image(systemName: presentation.systemImage)
                            .foregroundStyle(AgentAvatarPalette.tintColor(named: presentation.tintName))
                        VStack(alignment: .leading, spacing: 2) {
                            Text(presentation.title)
                                .font(.body.weight(.medium))
                            Text(presentation.subtitle)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }

                    if let errorSummary = env.desktopConnectionErrorSummary {
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
            .padding()
            .tabItem {
                Label("Connection", systemImage: "bolt.horizontal.circle")
            }
        }
        .frame(width: 540, height: 340)
        .onAppear {
            daemonURLDraft = workspaceStore.daemonURL
        }
    }

    private func applyDaemonURL() {
        let trimmed = daemonURLDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        env.updateDaemonURL(trimmed)
        Task {
            await LocalDaemonController.shared.ensureRunning(for: trimmed)
            await workspaceStore.refreshAgentsFromDaemon()
        }
    }
}