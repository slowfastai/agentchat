import SwiftUI

struct AgentChatDesktopSettingsView: View {
    @EnvironmentObject private var store: DaemonChatStore

    @State private var daemonURLDraft = ""

    private var presentation: AgentChatDesktopConnectionPresentation {
        AgentChatDesktopConnectionPresentation(state: store.connectionState)
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
                            .foregroundStyle(AgentAvatarPalette.tintColor(named: presentation.tintName))
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
            .padding()
            .tabItem {
                Label("Connection", systemImage: "bolt.horizontal.circle")
            }
        }
        .frame(width: 540, height: 340)
        .onAppear {
            daemonURLDraft = store.daemonURL
        }
        .onChange(of: store.daemonURL) { _, newValue in
            daemonURLDraft = newValue
        }
    }
}
