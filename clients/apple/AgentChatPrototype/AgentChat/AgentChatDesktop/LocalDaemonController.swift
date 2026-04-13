import Foundation
import Network
import OSLog

struct LocalDaemonLaunchCommand: Equatable {
    let executableURL: URL
    let arguments: [String]
    let currentDirectoryURL: URL?
}

private nonisolated final class LocalDaemonProbeState: @unchecked Sendable {
    private let lock = NSLock()
    private var didResume = false

    func beginFinish() -> Bool {
        lock.lock()
        defer { lock.unlock() }

        guard !didResume else {
            return false
        }

        didResume = true
        return true
    }
}

@MainActor
final class LocalDaemonController {
    static let shared = LocalDaemonController()

    private let logger = Logger(subsystem: "dev.slowfast.AgentChatDesktop", category: "LocalDaemon")
    private var daemonProcess: Process?
    private var didLaunchDaemon = false
    private var isStarting = false

    private init() {}

    func ensureRunning(for connectionLink: String) async {
        guard Self.shouldManageLocalDaemon(for: connectionLink) else {
            return
        }

        if let daemonProcess, daemonProcess.isRunning {
            return
        }

        if isStarting {
            return
        }

        if await Self.canConnectToLocalDaemon() {
            return
        }

        guard let launchCommand = Self.resolvedLaunchCommand() else {
            logger.error("Unable to resolve a local agentchat-daemon launch command")
            return
        }

        isStarting = true
        defer { isStarting = false }

        do {
            let process = Process()
            process.executableURL = launchCommand.executableURL
            process.arguments = launchCommand.arguments
            if let currentDirectoryURL = launchCommand.currentDirectoryURL {
                process.currentDirectoryURL = currentDirectoryURL
            }

            let outputPipe = Pipe()
            process.standardOutput = outputPipe
            process.standardError = outputPipe

            process.terminationHandler = { finishedProcess in
                let processID = finishedProcess.processIdentifier
                let exitCode = finishedProcess.terminationStatus
                Task { @MainActor in
                    LocalDaemonController.shared.handleTermination(processID: processID, exitCode: exitCode)
                }
            }

            try process.run()

            daemonProcess = process
            didLaunchDaemon = true
            logger.info(
                "Launched local daemon via \(launchCommand.executableURL.path(percentEncoded: false), privacy: .public)"
            )

            guard await Self.waitUntilDaemonIsReachable() else {
                logger.error("Local daemon process started but ws://127.0.0.1:9390 did not become reachable")
                return
            }

            logger.info("Local daemon is reachable on ws://127.0.0.1:9390")
        } catch {
            logger.error("Failed to launch local daemon: \(error.localizedDescription, privacy: .public)")
            daemonProcess = nil
            didLaunchDaemon = false
        }
    }

    func stopManagedDaemonIfNeeded() {
        guard didLaunchDaemon, let daemonProcess, daemonProcess.isRunning else {
            return
        }

        logger.info("Stopping local daemon launched by AgentChatDesktop")
        daemonProcess.terminate()
    }

    private func handleTermination(processID: Int32, exitCode: Int32) {
        guard daemonProcess?.processIdentifier == processID else {
            return
        }

        logger.info("Local daemon process exited with status \(exitCode, privacy: .public)")
        daemonProcess = nil
        didLaunchDaemon = false
    }

    nonisolated static func shouldManageLocalDaemon(for connectionLink: String) -> Bool {
        let trimmed = connectionLink.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return false
        }

        guard let payload = parseScannedDaemonConnectionPayload(from: trimmed) else {
            return false
        }

        switch payload {
        case .direct(let urlString, _):
            guard let url = URL(string: urlString),
                  let host = url.host?.lowercased() else {
                return false
            }
            return isLocalDaemonHost(host)
        case .relay:
            return false
        }
    }

    nonisolated static func resolvedLaunchCommand(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        sourceFilePath: String = #filePath,
        pathExists: (String) -> Bool = { FileManager.default.fileExists(atPath: $0) },
        executableExists: (String) -> Bool = { FileManager.default.isExecutableFile(atPath: $0) }
    ) -> LocalDaemonLaunchCommand? {
        if let override = environment["AGENTCHAT_DAEMON_EXECUTABLE"]?
            .trimmingCharacters(in: .whitespacesAndNewlines),
           !override.isEmpty {
            return launchCommand(
                executablePath: override,
                arguments: [],
                currentDirectoryURL: developmentRepoRoot(
                    sourceFilePath: sourceFilePath,
                    pathExists: pathExists
                )
            )
        }

        let repoRoot = developmentRepoRoot(sourceFilePath: sourceFilePath, pathExists: pathExists)
        let bundledAndInstalledCandidates = bundledExecutableCandidates()
            + installedExecutableCandidates()
            + developmentExecutableCandidates(repoRoot: repoRoot)

        for candidate in bundledAndInstalledCandidates where executableExists(candidate.path) {
            return LocalDaemonLaunchCommand(
                executableURL: candidate,
                arguments: [],
                currentDirectoryURL: repoRoot
            )
        }

        if let repoRoot {
            let manifestPath = repoRoot
                .appendingPathComponent("daemon", isDirectory: true)
                .appendingPathComponent("Cargo.toml")
                .path
            if pathExists(manifestPath) {
                return LocalDaemonLaunchCommand(
                    executableURL: URL(fileURLWithPath: "/usr/bin/env"),
                    arguments: [
                        "cargo",
                        "run",
                        "--manifest-path",
                        manifestPath,
                        "-p",
                        "agentchat-daemon",
                        "--bin",
                        "agentchat-daemon",
                    ],
                    currentDirectoryURL: repoRoot
                )
            }
        }

        return LocalDaemonLaunchCommand(
            executableURL: URL(fileURLWithPath: "/usr/bin/env"),
            arguments: ["agentchat-daemon"],
            currentDirectoryURL: repoRoot
        )
    }

    nonisolated static func developmentRepoRoot(
        sourceFilePath: String = #filePath,
        pathExists: (String) -> Bool = { FileManager.default.fileExists(atPath: $0) }
    ) -> URL? {
        var candidate = URL(fileURLWithPath: sourceFilePath)
            .deletingLastPathComponent()

        while candidate.path != "/" {
            let manifestPath = candidate
                .appendingPathComponent("daemon", isDirectory: true)
                .appendingPathComponent("Cargo.toml")
                .path
            if pathExists(manifestPath) {
                return candidate
            }
            candidate.deleteLastPathComponent()
        }

        return nil
    }

    private nonisolated static func launchCommand(
        executablePath: String,
        arguments: [String],
        currentDirectoryURL: URL?
    ) -> LocalDaemonLaunchCommand {
        if executablePath.contains("/") {
            return LocalDaemonLaunchCommand(
                executableURL: URL(fileURLWithPath: executablePath),
                arguments: arguments,
                currentDirectoryURL: currentDirectoryURL
            )
        }

        return LocalDaemonLaunchCommand(
            executableURL: URL(fileURLWithPath: "/usr/bin/env"),
            arguments: [executablePath] + arguments,
            currentDirectoryURL: currentDirectoryURL
        )
    }

    private nonisolated static func bundledExecutableCandidates(bundle: Bundle = .main) -> [URL] {
        var candidates: [URL] = []

        if let executableURL = bundle.executableURL {
            candidates.append(
                executableURL
                    .deletingLastPathComponent()
                    .appendingPathComponent("agentchat-daemon")
            )
        }

        if let sharedSupportURL = bundle.sharedSupportURL {
            candidates.append(sharedSupportURL.appendingPathComponent("agentchat-daemon"))
        }

        if let resourceURL = bundle.resourceURL {
            candidates.append(resourceURL.appendingPathComponent("agentchat-daemon"))
        }

        return candidates
    }

    private nonisolated static func installedExecutableCandidates(
        homeDirectoryPath: String = NSHomeDirectory()
    ) -> [URL] {
        [
            URL(fileURLWithPath: homeDirectoryPath)
                .appendingPathComponent(".cargo", isDirectory: true)
                .appendingPathComponent("bin", isDirectory: true)
                .appendingPathComponent("agentchat-daemon"),
            URL(fileURLWithPath: "/opt/homebrew/bin/agentchat-daemon"),
            URL(fileURLWithPath: "/usr/local/bin/agentchat-daemon"),
        ]
    }

    private nonisolated static func developmentExecutableCandidates(repoRoot: URL?) -> [URL] {
        guard let repoRoot else {
            return []
        }

        let targetRoot = repoRoot
            .appendingPathComponent("daemon", isDirectory: true)
            .appendingPathComponent("target", isDirectory: true)

        return [
            targetRoot.appendingPathComponent("debug", isDirectory: true).appendingPathComponent("agentchat-daemon"),
            targetRoot.appendingPathComponent("release", isDirectory: true).appendingPathComponent("agentchat-daemon"),
        ]
    }

    private nonisolated static func isLocalDaemonHost(_ host: String) -> Bool {
        switch host {
        case "127.0.0.1", "localhost", "::1", "0.0.0.0":
            return true
        default:
            return false
        }
    }

    private static func canConnectToLocalDaemon(timeoutSeconds: TimeInterval = 0.35) async -> Bool {
        await withCheckedContinuation { continuation in
            let connection = NWConnection(host: "127.0.0.1", port: 9390, using: .tcp)
            let queue = DispatchQueue(label: "dev.slowfast.AgentChatDesktop.LocalDaemonProbe")
            let probeState = LocalDaemonProbeState()

            @Sendable func finish(_ result: Bool) {
                guard probeState.beginFinish() else { return }
                connection.stateUpdateHandler = nil
                connection.cancel()
                continuation.resume(returning: result)
            }

            connection.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    finish(true)
                case .failed, .cancelled:
                    finish(false)
                default:
                    break
                }
            }

            connection.start(queue: queue)
            queue.asyncAfter(deadline: .now() + timeoutSeconds) {
                finish(false)
            }
        }
    }

    private static func waitUntilDaemonIsReachable(
        timeoutSeconds: TimeInterval = 8,
        pollIntervalNanoseconds: UInt64 = 250_000_000
    ) async -> Bool {
        let deadline = Date().addingTimeInterval(timeoutSeconds)

        while Date() < deadline {
            if await canConnectToLocalDaemon() {
                return true
            }
            try? await Task.sleep(nanoseconds: pollIntervalNanoseconds)
        }

        return await canConnectToLocalDaemon(timeoutSeconds: 0.75)
    }
}
