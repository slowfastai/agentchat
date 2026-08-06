import Foundation
import Testing
@testable import AgentChatDesktop

struct LocalDaemonControllerTests {
    @Test func localDaemonEnvironmentPrependsPreferredCliPaths() {
        let environment = LocalDaemonEnvironment.make(
            from: ["PATH": "/usr/bin:/custom/bin:/opt/homebrew/bin"],
            homeDirectoryPath: "/Users/tester"
        )

        #expect(environment.values["PATH"] == [
            "/Users/tester/.opencode/bin",
            "/Users/tester/.cargo/bin",
            "/opt/homebrew/bin",
            "/opt/homebrew/sbin",
            "/usr/local/bin",
            "/usr/local/sbin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
            "/Library/Apple/usr/bin",
            "/custom/bin",
        ].joined(separator: ":"))
        #expect(environment.values["HOME"] == "/Users/tester")
        #expect(environment.values["AGENTCHAT_HOME"] == "/Users/tester/Library/Application Support/AgentChat")
        #expect(environment.values["AGENTCHAT_AGENTS_FILE"] == "/Users/tester/Library/Application Support/AgentChat/config/agents.json")
    }

    @Test func localDaemonEnvironmentPreservesExplicitAgentConfiguration() {
        let environment = LocalDaemonEnvironment.make(
            from: [
                "PATH": "/usr/bin",
                "AGENTCHAT_HOME": "/tmp/custom-agentchat",
                "AGENTCHAT_AGENTS_FILE": "/tmp/custom-agentchat/agents.json",
                "AGENTCHAT_AGENT_COMMAND": "opencode",
            ],
            homeDirectoryPath: "/Users/tester"
        )

        #expect(environment.values["AGENTCHAT_AGENT_COMMAND"] == "opencode")
        #expect(environment.values["AGENTCHAT_HOME"] == "/tmp/custom-agentchat")
        #expect(environment.values["AGENTCHAT_AGENTS_FILE"] == "/tmp/custom-agentchat/agents.json")
    }

    @Test func localDaemonEnvironmentPreservesExplicitCodexHome() {
        let environment = LocalDaemonEnvironment.make(
            from: [
                "PATH": "/usr/bin",
                "CODEX_HOME": "/Users/tester/.codex-work",
            ],
            homeDirectoryPath: "/Users/tester"
        )

        #expect(environment.values["CODEX_HOME"] == "/Users/tester/.codex-work")
        #expect(environment.values["HOME"] == "/Users/tester")
    }

    @Test func defaultManagedAgentsJSONIncludesMultipleBuiltInAgents() {
        let json = LocalDaemonEnvironment.defaultManagedAgentsJSON()

        #expect(json.contains("\"id\": \"codex\""))
        #expect(json.contains("\"id\": \"opencode\""))
        #expect(json.contains("\"id\": \"claude-code\""))
        #expect(json.contains("\"id\": \"pi\""))
    }

    @Test func defaultManagedAgentsJSONResolvesCodexFromNodeCellarWhenPathMissesIt() {
        let json = LocalDaemonEnvironment.defaultManagedAgentsJSON(
            homeDirectoryPath: "/Users/tester",
            baseEnvironment: ["PATH": "/usr/bin:/opt/homebrew/bin"],
            executableExists: { path in
                path == "/opt/homebrew/Cellar/node/23.11.0/bin/codex"
                    || path == "/usr/bin/npx"
            },
            directoryContents: { path in
                if path == "/opt/homebrew/Cellar/node" {
                    return ["23.11.0"]
                }
                return []
            }
        )

        #expect(json.contains("\"command\": \"/opt/homebrew/Cellar/node/23.11.0/bin/codex\""))
    }

    @Test func localDaemonManagementOnlyActivatesForLoopbackDirectLinks() {
        #expect(LocalDaemonController.shouldManageLocalDaemon(for: "ws://127.0.0.1:9390"))
        #expect(LocalDaemonController.shouldManageLocalDaemon(for: "ws://localhost:9390"))
        #expect(LocalDaemonController.shouldManageLocalDaemon(
            for: "agentchat://connect?url=ws%3A%2F%2F127.0.0.1%3A9390"
        ))

        #expect(!LocalDaemonController.shouldManageLocalDaemon(for: "ws://192.168.1.20:9390"))
        #expect(!LocalDaemonController.shouldManageLocalDaemon(
            for: "agentchat://connect?relay_url=wss%3A%2F%2Frelay.agentchat.dev%2Fv1%2Fws&device_id=dev_local_1&relay_pairing=dev&relay_crypto=dev"
        ))
    }

    @Test func developmentRepoRootWalksUpToWorkspaceRoot() {
        let sourceFilePath = "/tmp/agentchat/clients/apple/AgentChatPrototype/AgentChat/AgentChatDesktop/LocalDaemonController.swift"
        let repoRoot = LocalDaemonController.developmentRepoRoot(
            sourceFilePath: sourceFilePath,
            pathExists: { path in
                path == "/tmp/agentchat/daemon/Cargo.toml"
            }
        )

        #expect(repoRoot?.path == "/tmp/agentchat")
    }

    @Test func resolvedLaunchCommandPrefersExplicitEnvironmentOverride() {
        let command = LocalDaemonController.resolvedLaunchCommand(
            environment: ["AGENTCHAT_DAEMON_EXECUTABLE": "/custom/bin/agentchat-daemon"],
            sourceFilePath: "/tmp/agentchat/clients/apple/AgentChatPrototype/AgentChat/AgentChatDesktop/LocalDaemonController.swift",
            pathExists: { path in
                path == "/tmp/agentchat/daemon/Cargo.toml"
            },
            executableExists: { _ in false }
        )

        #expect(command?.executableURL.path == "/custom/bin/agentchat-daemon")
        #expect(command?.arguments == [])
        #expect(command?.currentDirectoryURL?.path == "/tmp/agentchat")
    }

    @Test func resolvedLaunchCommandUsesRepoDebugBinaryWhenAvailable() {
        let sourceFilePath = "/tmp/agentchat/clients/apple/AgentChatPrototype/AgentChat/AgentChatDesktop/LocalDaemonController.swift"
        let debugBinaryPath = "/tmp/agentchat/daemon/target/debug/agentchat-daemon"

        let command = LocalDaemonController.resolvedLaunchCommand(
            environment: [:],
            sourceFilePath: sourceFilePath,
            pathExists: { path in
                path == "/tmp/agentchat/daemon/Cargo.toml"
            },
            executableExists: { path in
                path == debugBinaryPath
            }
        )

        #expect(command?.executableURL.path == debugBinaryPath)
        #expect(command?.arguments == [])
        #expect(command?.currentDirectoryURL?.path == "/tmp/agentchat")
    }

    @Test func resolvedLaunchCommandFallsBackToCargoRunInsideRepo() {
        let sourceFilePath = "/tmp/agentchat/clients/apple/AgentChatPrototype/AgentChat/AgentChatDesktop/LocalDaemonController.swift"
        let manifestPath = "/tmp/agentchat/daemon/Cargo.toml"

        let command = LocalDaemonController.resolvedLaunchCommand(
            environment: [:],
            sourceFilePath: sourceFilePath,
            pathExists: { path in
                path == manifestPath
            },
            executableExists: { _ in false }
        )

        #expect(command?.executableURL.path == "/usr/bin/env")
        #expect(command?.arguments == [
            "cargo",
            "run",
            "--manifest-path",
            manifestPath,
            "-p",
            "agentchat-daemon",
            "--bin",
            "agentchat-daemon",
        ])
        #expect(command?.currentDirectoryURL?.path == "/tmp/agentchat")
    }

    @Test func resolvedWebLaunchCommandAddsWebArgumentsToBundledBinary() {
        let command = LocalDaemonController.resolvedWebLaunchCommand(
            environment: ["AGENTCHAT_DAEMON_EXECUTABLE": "/custom/bin/agentchat-daemon"],
            sourceFilePath: "/tmp/agentchat/clients/apple/AgentChatPrototype/AgentChat/AgentChatDesktop/LocalDaemonController.swift",
            pathExists: { _ in false },
            executableExists: { _ in false }
        )

        #expect(command?.executableURL.path == "/custom/bin/agentchat-daemon")
        #expect(command?.arguments == ["web", "--port", "9391"])
    }

    @Test func resolvedWebLaunchCommandPassesArgumentsThroughCargoRun() {
        let sourceFilePath = "/tmp/agentchat/clients/apple/AgentChatPrototype/AgentChat/AgentChatDesktop/LocalDaemonController.swift"
        let manifestPath = "/tmp/agentchat/daemon/Cargo.toml"

        let command = LocalDaemonController.resolvedWebLaunchCommand(
            environment: [:],
            sourceFilePath: sourceFilePath,
            pathExists: { path in path == manifestPath },
            executableExists: { _ in false }
        )

        #expect(command?.executableURL.path == "/usr/bin/env")
        #expect(command?.arguments == [
            "cargo",
            "run",
            "--manifest-path",
            manifestPath,
            "-p",
            "agentchat-daemon",
            "--bin",
            "agentchat-daemon",
            "--",
            "web",
            "--port",
            "9391",
        ])
    }

    @Test func resolvedInstallableDaemonBinaryURLRejectsCargoRunFallback() {
        let sourceFilePath = "/tmp/agentchat/clients/apple/AgentChatPrototype/AgentChat/AgentChatDesktop/LocalDaemonController.swift"
        let installableURL = LocalDaemonController.resolvedInstallableDaemonBinaryURL(
            environment: [:],
            sourceFilePath: sourceFilePath,
            pathExists: { path in
                path == "/tmp/agentchat/daemon/Cargo.toml"
            },
            executableExists: { _ in false }
        )

        #expect(installableURL == nil)
    }

    @Test func launchAgentLayoutUsesPerUserApplicationSupportPaths() {
        let layout = LocalDaemonInstallLayout.make(
            homeDirectoryURL: URL(fileURLWithPath: "/Users/tester", isDirectory: true)
        )

        #expect(layout.agentChatHomeURL.path == "/Users/tester/Library/Application Support/AgentChat")
        #expect(layout.daemonBinaryURL.path == "/Users/tester/Library/Application Support/AgentChat/bin/agentchat-daemon")
        #expect(layout.agentsFileURL.path == "/Users/tester/Library/Application Support/AgentChat/config/agents.json")
        #expect(layout.launchAgentPlistURL.path == "/Users/tester/Library/LaunchAgents/dev.slowfast.agentchat.daemon.plist")
    }

    @Test func launchAgentPlistIncludesStablePathsAndEnvironment() {
        let layout = LocalDaemonInstallLayout.make(
            homeDirectoryURL: URL(fileURLWithPath: "/Users/tester", isDirectory: true)
        )
        let environment = LocalDaemonEnvironment.make(
            from: ["PATH": "/usr/bin"],
            homeDirectoryPath: "/Users/tester",
            installLayout: layout
        ).values
        let plist = makeLaunchAgentPlist(layout: layout, environment: environment)

        #expect(plist.contains("dev.slowfast.agentchat.daemon"))
        #expect(plist.contains("/Users/tester/Library/Application Support/AgentChat/bin/agentchat-daemon"))
        #expect(plist.contains("/Users/tester/Library/Application Support/AgentChat/config/agents.json"))
        #expect(plist.contains("/Users/tester/Library/Application Support/AgentChat/logs/daemon.stdout.log"))
        #expect(plist.contains("<key>HOME</key>"))
        #expect(plist.contains("<key>KeepAlive</key>"))
    }

    @Test func launchAgentPlistCarriesCodexHomeButNotSecretEnvironmentValues() {
        let layout = LocalDaemonInstallLayout.make(
            homeDirectoryURL: URL(fileURLWithPath: "/Users/tester", isDirectory: true)
        )
        let environment = LocalDaemonEnvironment.make(
            from: [
                "PATH": "/usr/bin",
                "CODEX_HOME": "/Users/tester/.codex-work",
                "OPENAI_API_KEY": "should-not-be-written",
            ],
            homeDirectoryPath: "/Users/tester",
            installLayout: layout
        ).values
        let plist = makeLaunchAgentPlist(layout: layout, environment: environment)

        #expect(plist.contains("<key>CODEX_HOME</key>"))
        #expect(plist.contains("/Users/tester/.codex-work"))
        #expect(!plist.contains("OPENAI_API_KEY"))
        #expect(!plist.contains("should-not-be-written"))
    }

    @Test func webLaunchAgentPlistStartsTheWebConsole() {
        let layout = LocalDaemonInstallLayout.make(
            homeDirectoryURL: URL(fileURLWithPath: "/Users/tester", isDirectory: true)
        )
        let plist = makeLaunchAgentPlist(
            layout: layout,
            environment: [:],
            daemonArguments: LocalDaemonController.webDaemonArguments
        )

        #expect(plist.contains("<string>web</string>"))
        #expect(plist.contains("<string>--port</string>"))
        #expect(plist.contains("<string>9391</string>"))
    }
}
