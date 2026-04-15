import Foundation
import Network
import OSLog
import Darwin

struct LocalDaemonLaunchCommand: Equatable {
    let executableURL: URL
    let arguments: [String]
    let currentDirectoryURL: URL?
}

struct LocalDaemonInstallLayout: Equatable {
    nonisolated static let launchAgentLabel = "dev.slowfast.agentchat.daemon"

    let homeDirectoryURL: URL
    let agentChatHomeURL: URL
    let binDirectoryURL: URL
    let daemonBinaryURL: URL
    let configDirectoryURL: URL
    let agentsFileURL: URL
    let logsDirectoryURL: URL
    let stdoutLogURL: URL
    let stderrLogURL: URL
    let launchAgentsDirectoryURL: URL
    let launchAgentPlistURL: URL

    nonisolated static func make(homeDirectoryURL: URL = FileManager.default.homeDirectoryForCurrentUser) -> LocalDaemonInstallLayout {
        let applicationSupportURL = homeDirectoryURL
            .appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("Application Support", isDirectory: true)
            .appendingPathComponent("AgentChat", isDirectory: true)
        let binDirectoryURL = applicationSupportURL.appendingPathComponent("bin", isDirectory: true)
        let configDirectoryURL = applicationSupportURL.appendingPathComponent("config", isDirectory: true)
        let logsDirectoryURL = applicationSupportURL.appendingPathComponent("logs", isDirectory: true)
        let launchAgentsDirectoryURL = homeDirectoryURL
            .appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("LaunchAgents", isDirectory: true)

        return LocalDaemonInstallLayout(
            homeDirectoryURL: homeDirectoryURL,
            agentChatHomeURL: applicationSupportURL,
            binDirectoryURL: binDirectoryURL,
            daemonBinaryURL: binDirectoryURL.appendingPathComponent("agentchat-daemon"),
            configDirectoryURL: configDirectoryURL,
            agentsFileURL: configDirectoryURL.appendingPathComponent("agents.json"),
            logsDirectoryURL: logsDirectoryURL,
            stdoutLogURL: logsDirectoryURL.appendingPathComponent("daemon.stdout.log"),
            stderrLogURL: logsDirectoryURL.appendingPathComponent("daemon.stderr.log"),
            launchAgentsDirectoryURL: launchAgentsDirectoryURL,
            launchAgentPlistURL: launchAgentsDirectoryURL.appendingPathComponent("\(launchAgentLabel).plist")
        )
    }

    nonisolated var workingDirectoryURL: URL {
        agentChatHomeURL
    }

    nonisolated var launchDomain: String {
        "gui/\(getuid())"
    }

    nonisolated var launchServiceTarget: String {
        "\(launchDomain)/\(Self.launchAgentLabel)"
    }
}

struct LocalDaemonEnvironment {
    let values: [String: String]

    nonisolated static func make(
        from base: [String: String] = ProcessInfo.processInfo.environment,
        homeDirectoryPath: String = NSHomeDirectory(),
        installLayout: LocalDaemonInstallLayout = .make()
    ) -> LocalDaemonEnvironment {
        var values = base
        values["PATH"] = launchPath(
            existingPath: base["PATH"],
            homeDirectoryPath: homeDirectoryPath
        )
        values["AGENTCHAT_HOME"] = nonEmptyValue(values["AGENTCHAT_HOME"]) ?? installLayout.agentChatHomeURL.path
        values["AGENTCHAT_AGENTS_FILE"] = nonEmptyValue(values["AGENTCHAT_AGENTS_FILE"]) ?? installLayout.agentsFileURL.path
        return LocalDaemonEnvironment(values: values)
    }

    nonisolated static let defaultManagedAgentsJSON = """
    [
      {
        "id": "codex",
        "name": "Codex",
        "backend": "codex_app_server",
        "command": "codex",
        "args": []
      }
    ]
    """

    private nonisolated static func nonEmptyValue(_ value: String?) -> String? {
        guard let trimmed = value?.trimmingCharacters(in: .whitespacesAndNewlines),
              !trimmed.isEmpty else {
            return nil
        }
        return trimmed
    }

    private nonisolated static func launchPath(existingPath: String?, homeDirectoryPath: String) -> String {
        let preferredComponents = [
            homeDirectoryPath + "/.opencode/bin",
            homeDirectoryPath + "/.cargo/bin",
            "/opt/homebrew/bin",
            "/opt/homebrew/sbin",
            "/usr/local/bin",
            "/usr/local/sbin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
            "/Library/Apple/usr/bin",
        ]

        let existingComponents = (existingPath ?? "")
            .split(separator: ":")
            .map(String.init)

        var orderedComponents: [String] = []
        for component in preferredComponents + existingComponents {
            let trimmed = component.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty, !orderedComponents.contains(trimmed) else {
                continue
            }
            orderedComponents.append(trimmed)
        }

        return orderedComponents.joined(separator: ":")
    }
}

private final class LocalDaemonProbeState: @unchecked Sendable {
    private let lock = NSLock()
    nonisolated(unsafe) private var didResume = false

    nonisolated func beginFinish() -> Bool {
        lock.lock()
        defer { lock.unlock() }

        guard !didResume else {
            return false
        }

        didResume = true
        return true
    }
}

enum LocalDaemonLaunchAgentError: LocalizedError {
    case missingInstallableBinary
    case launchctlFailed(arguments: [String], output: String)

    var errorDescription: String? {
        switch self {
        case .missingInstallableBinary:
            return "No installable agentchat-daemon binary was found for LaunchAgent setup."
        case .launchctlFailed(let arguments, let output):
            return "launchctl \(arguments.joined(separator: " ")) failed: \(output)"
        }
    }
}

private nonisolated func xmlEscaped(_ value: String) -> String {
    value
        .replacingOccurrences(of: "&", with: "&amp;")
        .replacingOccurrences(of: "<", with: "&lt;")
        .replacingOccurrences(of: ">", with: "&gt;")
        .replacingOccurrences(of: "\"", with: "&quot;")
}

nonisolated func makeLaunchAgentPlist(
    layout: LocalDaemonInstallLayout,
    environment: [String: String]
) -> String {
    let path = xmlEscaped(environment["PATH"] ?? "")
    let agentChatHome = xmlEscaped(environment["AGENTCHAT_HOME"] ?? layout.agentChatHomeURL.path)
    let agentsFile = xmlEscaped(environment["AGENTCHAT_AGENTS_FILE"] ?? layout.agentsFileURL.path)
    let daemonBinary = xmlEscaped(layout.daemonBinaryURL.path)
    let workingDirectory = xmlEscaped(layout.workingDirectoryURL.path)
    let stdoutPath = xmlEscaped(layout.stdoutLogURL.path)
    let stderrPath = xmlEscaped(layout.stderrLogURL.path)

    return """
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0">
    <dict>
      <key>Label</key>
      <string>\(LocalDaemonInstallLayout.launchAgentLabel)</string>
      <key>ProgramArguments</key>
      <array>
        <string>\(daemonBinary)</string>
      </array>
      <key>WorkingDirectory</key>
      <string>\(workingDirectory)</string>
      <key>RunAtLoad</key>
      <true/>
      <key>KeepAlive</key>
      <true/>
      <key>EnvironmentVariables</key>
      <dict>
        <key>PATH</key>
        <string>\(path)</string>
        <key>AGENTCHAT_HOME</key>
        <string>\(agentChatHome)</string>
        <key>AGENTCHAT_AGENTS_FILE</key>
        <string>\(agentsFile)</string>
      </dict>
      <key>StandardOutPath</key>
      <string>\(stdoutPath)</string>
      <key>StandardErrorPath</key>
      <string>\(stderrPath)</string>
    </dict>
    </plist>
    """
}

@MainActor
final class LocalDaemonController {
    static let shared = LocalDaemonController()

    private let logger = Logger(subsystem: "dev.slowfast.AgentChatDesktop", category: "LocalDaemon")
    private var daemonProcess: Process?
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

        guard !isStarting else {
            return
        }

        if let daemonProcess, daemonProcess.isRunning {
            return
        }

        isStarting = true
        defer { isStarting = false }

        do {
            if try await installAndStartLaunchAgentIfPossible() {
                logger.info("Started local daemon via launchd LaunchAgent")
            } else {
                try await launchDevelopmentChildProcess()
            }

            guard await Self.waitUntilDaemonIsReachable() else {
                logger.error("Local daemon process started but ws://127.0.0.1:9390 did not become reachable")
                return
            }

            logger.info("Local daemon is reachable on ws://127.0.0.1:9390")
        } catch {
            logger.error("Failed to launch local daemon: \(error.localizedDescription, privacy: .public)")
            daemonProcess = nil
        }
    }

    func stopManagedDaemonIfNeeded() {
        guard let daemonProcess, daemonProcess.isRunning else {
            return
        }

        logger.info("Stopping dev-only local daemon child process launched by AgentChatDesktop")
        daemonProcess.terminate()
    }

    private func handleTermination(processID: Int32, exitCode: Int32) {
        guard daemonProcess?.processIdentifier == processID else {
            return
        }

        logger.info("Local daemon process exited with status \(exitCode, privacy: .public)")
        daemonProcess = nil
    }

    private func installAndStartLaunchAgentIfPossible() async throws -> Bool {
        guard let installableBinaryURL = Self.resolvedInstallableDaemonBinaryURL() else {
            return false
        }

        let layout = LocalDaemonInstallLayout.make()
        let environment = LocalDaemonEnvironment.make(
            homeDirectoryPath: layout.homeDirectoryURL.path,
            installLayout: layout
        ).values

        try Self.installManagedDaemon(
            from: installableBinaryURL,
            layout: layout,
            environment: environment
        )
        try Self.bootstrapManagedDaemon(layout: layout)
        return true
    }

    private func launchDevelopmentChildProcess() async throws {
        guard let launchCommand = Self.resolvedLaunchCommand() else {
            logger.error("Unable to resolve a local agentchat-daemon launch command")
            return
        }

        let layout = LocalDaemonInstallLayout.make()
        let process = Process()
        process.executableURL = launchCommand.executableURL
        process.arguments = launchCommand.arguments
        process.environment = LocalDaemonEnvironment.make(
            homeDirectoryPath: layout.homeDirectoryURL.path,
            installLayout: layout
        ).values
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
        logger.info(
            "Launched dev-only local daemon child via \(launchCommand.executableURL.path(percentEncoded: false), privacy: .public)"
        )
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

    nonisolated static func resolvedInstallableDaemonBinaryURL(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        sourceFilePath: String = #filePath,
        pathExists: (String) -> Bool = { FileManager.default.fileExists(atPath: $0) },
        executableExists: (String) -> Bool = { FileManager.default.isExecutableFile(atPath: $0) }
    ) -> URL? {
        let command = resolvedLaunchCommand(
            environment: environment,
            sourceFilePath: sourceFilePath,
            pathExists: pathExists,
            executableExists: executableExists
        )

        guard let command,
              command.arguments.isEmpty,
              command.executableURL.path != "/usr/bin/env" else {
            return nil
        }

        return command.executableURL
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

    private nonisolated static func installManagedDaemon(
        from sourceBinaryURL: URL,
        layout: LocalDaemonInstallLayout,
        environment: [String: String],
        fileManager: FileManager = .default
    ) throws {
        for directory in [
            layout.agentChatHomeURL,
            layout.binDirectoryURL,
            layout.configDirectoryURL,
            layout.logsDirectoryURL,
            layout.launchAgentsDirectoryURL,
        ] {
            try fileManager.createDirectory(
                at: directory,
                withIntermediateDirectories: true,
                attributes: nil
            )
        }

        if !fileManager.fileExists(atPath: layout.agentsFileURL.path) {
            try LocalDaemonEnvironment.defaultManagedAgentsJSON.write(
                to: layout.agentsFileURL,
                atomically: true,
                encoding: .utf8
            )
        }

        try installBinaryAtomically(
            from: sourceBinaryURL,
            to: layout.daemonBinaryURL,
            fileManager: fileManager
        )

        let plist = makeLaunchAgentPlist(layout: layout, environment: environment)
        try plist.write(to: layout.launchAgentPlistURL, atomically: true, encoding: .utf8)
    }

    private nonisolated static func installBinaryAtomically(
        from sourceBinaryURL: URL,
        to destinationURL: URL,
        fileManager: FileManager = .default
    ) throws {
        if sourceBinaryURL.standardizedFileURL == destinationURL.standardizedFileURL {
            try fileManager.setAttributes(
                [.posixPermissions: 0o755],
                ofItemAtPath: destinationURL.path
            )
            return
        }

        let temporaryURL = destinationURL
            .deletingLastPathComponent()
            .appendingPathComponent("\(destinationURL.lastPathComponent).tmp")

        if fileManager.fileExists(atPath: temporaryURL.path) {
            try fileManager.removeItem(at: temporaryURL)
        }

        try fileManager.copyItem(at: sourceBinaryURL, to: temporaryURL)
        try fileManager.setAttributes([.posixPermissions: 0o755], ofItemAtPath: temporaryURL.path)

        if fileManager.fileExists(atPath: destinationURL.path) {
            _ = try fileManager.replaceItemAt(destinationURL, withItemAt: temporaryURL)
        } else {
            try fileManager.moveItem(at: temporaryURL, to: destinationURL)
        }
    }

    private nonisolated static func bootstrapManagedDaemon(layout: LocalDaemonInstallLayout) throws {
        do {
            try runLaunchctl(arguments: [
                "bootstrap",
                layout.launchDomain,
                layout.launchAgentPlistURL.path,
            ])
        } catch let error as LocalDaemonLaunchAgentError {
            let alreadyLoaded = error.localizedDescription.localizedCaseInsensitiveContains("already loaded")
                || error.localizedDescription.localizedCaseInsensitiveContains("already bootstrapped")
                || error.localizedDescription.localizedCaseInsensitiveContains("service already exists")
            if !alreadyLoaded {
                throw error
            }
        }

        try runLaunchctl(arguments: [
            "kickstart",
            "-k",
            layout.launchServiceTarget,
        ])
    }

    private nonisolated static func runLaunchctl(arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/launchctl")
        process.arguments = arguments
        let outputPipe = Pipe()
        process.standardOutput = outputPipe
        process.standardError = outputPipe

        try process.run()
        process.waitUntilExit()

        let data = outputPipe.fileHandleForReading.readDataToEndOfFile()
        let output = String(data: data, encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""

        guard process.terminationStatus == 0 else {
            throw LocalDaemonLaunchAgentError.launchctlFailed(arguments: arguments, output: output)
        }
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
