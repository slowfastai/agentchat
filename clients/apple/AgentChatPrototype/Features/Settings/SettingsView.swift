import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var store: DemoStore

    var body: some View {
        List {
            Section {
                DaemonStatusCard(
                    statusText: store.daemonStatusText,
                    accent: store.daemonStatusAccent,
                    isRefreshing: store.isRefreshingAgentsFromDaemon,
                    onRefresh: {
                        Task {
                            await store.refreshAgentsFromDaemon()
                        }
                    },
                    onReset: {
                        store.resetPrototypeData()
                    }
                )
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(Color.appCanvasBackground)
        .navigationTitle("Settings")
    }
}

private struct DaemonStatusCard: View {
    let statusText: String
    let accent: ColorToken
    let isRefreshing: Bool
    let onRefresh: () -> Void
    let onReset: () -> Void

    var body: some View {
        CardSurface(accent: accent) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .center, spacing: 12) {
                    Circle()
                        .fill(accent.color)
                        .frame(width: 10, height: 10)

                    VStack(alignment: .leading, spacing: 4) {
                        Text("Local Daemon")
                            .font(.subheadline.weight(.semibold))
                        Text(statusText)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    Spacer()

                    Button {
                        onRefresh()
                    } label: {
                        if isRefreshing {
                            ProgressView()
                                .controlSize(.small)
                        } else {
                            Label("Refresh", systemImage: "arrow.clockwise")
                        }
                    }
                    .buttonStyle(.bordered)
                    .disabled(isRefreshing)
                }

                HStack {
                    Spacer()
                    Button("Reset Prototype Data") {
                        onReset()
                    }
                    .buttonStyle(.bordered)
                }
            }
        }
    }
}