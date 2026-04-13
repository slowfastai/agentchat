import Foundation
import Testing
@testable import AgentChatDesktop

struct LocalDaemonControllerTests {
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
}
