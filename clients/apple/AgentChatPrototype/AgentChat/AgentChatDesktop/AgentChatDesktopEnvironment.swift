import Combine
import Foundation
import SwiftUI

@MainActor
final class DesktopEnvironment: ObservableObject {
    let workspace = WorkspaceStore()
    private let daemon = DaemonChatStore()
    private var cancellables = Set<AnyCancellable>()

    @Published var connectionState: DaemonConnectionState = .notConfigured

    var hasConfiguredDaemonURL: Bool { daemon.hasConfiguredDaemonURL }
    var desktopConnectionErrorSummary: String? { daemon.desktopConnectionErrorSummary }

    func start() {
        workspace.$daemonURL
            .dropFirst()
            .sink { [weak self] url in
                self?.daemon.updateDaemonURL(url)
                Task { await LocalDaemonController.shared.ensureWebRunning(for: url) }
            }
            .store(in: &cancellables)

        daemon.$connectionState
            .sink { [weak self] state in self?.connectionState = state }
            .store(in: &cancellables)

        daemon.updateDaemonURL(workspace.daemonURL)
        Task {
            await LocalDaemonController.shared.ensureWebRunning(for: workspace.daemonURL)
            daemon.start()
            await workspace.refreshAgentsFromDaemon()
            await NotificationHelper.requestAuthorization()
        }
    }

    func reconnectNow() {
        daemon.reconnectNow()
        Task { await workspace.refreshAgentsFromDaemon() }
    }

    func disconnect() { daemon.disconnect() }

    func applyScannedConnectionPayload(_ url: String) {
        daemon.applyScannedConnectionPayload(url)
    }

    func attachThread(_ threadID: String) {
        daemon.attachThread(threadID)
    }

    func updateDaemonURL(_ url: String) {
        workspace.updateDaemonURL(url)
    }
}
