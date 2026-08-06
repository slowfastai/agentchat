import AppKit
import SwiftUI
import WebKit

struct WebChatWindowView: View {
    @State private var consoleURL: URL?
    @State private var errorMessage: String?
    @State private var isStarting = false
    @State private var launchAttempt = 0

    var body: some View {
        Group {
            if let consoleURL {
                WebChatWebView(url: consoleURL)
            } else if let errorMessage {
                VStack(spacing: 14) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.system(size: 28))
                        .foregroundStyle(.orange)
                    Text("AgentChat Web could not start")
                        .font(.title3.weight(.semibold))
                    Text(errorMessage)
                        .multilineTextAlignment(.center)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: 480)
                    Button("Retry") {
                        self.errorMessage = nil
                        self.launchAttempt += 1
                    }
                    .buttonStyle(.borderedProminent)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .padding(32)
            } else {
                VStack(spacing: 14) {
                    ProgressView()
                    Text(isStarting ? "Starting local AgentChat daemon..." : "Preparing AgentChat Web...")
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(minWidth: 1080, minHeight: 720)
        .task(id: launchAttempt) {
            await startWebConsoleIfNeeded()
        }
    }

    @MainActor
    private func startWebConsoleIfNeeded() async {
        guard consoleURL == nil else { return }

        isStarting = true
        defer { isStarting = false }

        let ready = await LocalDaemonController.shared.ensureWebRunning(
            for: "ws://127.0.0.1:\(LocalDaemonController.webSocketPort)"
        )
        guard ready else {
            errorMessage = "The local web daemon did not become reachable at \(LocalDaemonController.webConsoleURL.absoluteString)."
            return
        }

        consoleURL = LocalDaemonController.webConsoleURL
    }
}

struct WebChatWebView: NSViewRepresentable {
    private static let notificationHandlerName = "agentchatSystemNotification"

    let url: URL

    func makeNSView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .default()
        configuration.userContentController.add(
            context.coordinator,
            name: Self.notificationHandlerName
        )
        let webView = WKWebView(frame: .zero, configuration: configuration)
        webView.navigationDelegate = context.coordinator
        webView.allowsMagnification = true
        webView.load(URLRequest(url: url))
        return webView
    }

    func updateNSView(_ webView: WKWebView, context: Context) {
        guard webView.url?.absoluteString != url.absoluteString else { return }
        webView.load(URLRequest(url: url))
    }

    func makeCoordinator() -> NavigationCoordinator {
        NavigationCoordinator()
    }

    final class NavigationCoordinator: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
        func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
            guard message.name == WebChatWebView.notificationHandlerName,
                  let payload = message.body as? [String: Any],
                  let threadID = payload["thread_id"] as? String,
                  let agentName = payload["agent_name"] as? String else {
                return
            }

            NotificationHelper.sendAgentResponseNotification(
                agentName: agentName,
                message: payload["response"] as? String ?? "",
                threadID: threadID,
                eventID: payload["event_id"] as? String
            )
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            NSLog("AgentChat Web navigation failed: %@", error.localizedDescription)
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            NSLog("AgentChat Web provisional navigation failed: %@", error.localizedDescription)
        }
    }
}
