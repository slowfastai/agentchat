import SwiftUI

@main
struct AgentChatPrototypeApp: App {
    @StateObject private var store = DemoStore()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(store)
        }
    }
}
