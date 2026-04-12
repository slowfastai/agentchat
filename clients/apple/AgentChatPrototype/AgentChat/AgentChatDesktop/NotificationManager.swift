import Foundation
import UserNotifications
import AppKit

enum NotificationHelper {
    static var isAuthorized = false

    static func requestAuthorization() async {
        let center = UNUserNotificationCenter.current()
        do {
            let granted = try await center.requestAuthorization(options: [.alert, .sound, .badge])
            await MainActor.run {
                self.isAuthorized = granted
            }
        } catch {
            await MainActor.run {
                self.isAuthorized = false
            }
        }
    }

    static func checkAuthorizationStatus() async {
        let center = UNUserNotificationCenter.current()
        let settings = await center.notificationSettings()
        await MainActor.run {
            self.isAuthorized = settings.authorizationStatus == .authorized
        }
    }

    static func sendNotification(title: String, body: String, threadID: String? = nil) {
        guard isAuthorized else { return }

        DispatchQueue.main.async {
            let content = UNMutableNotificationContent()
            content.title = title
            content.body = body
            content.sound = .default

            if let threadID = threadID {
                content.userInfo = ["threadID": threadID]
                content.categoryIdentifier = "THREAD_MESSAGE"
            }

            let request = UNNotificationRequest(
                identifier: UUID().uuidString,
                content: content,
                trigger: nil
            )

            UNUserNotificationCenter.current().add(request) { error in
                if let error {
                    print("Notification error: \(error.localizedDescription)")
                }
            }
        }
    }

    static func sendAgentResponseNotification(agentName: String, message: String, threadID: String) {
        let title = agentName
        let body = message.prefix(100).trimmingCharacters(in: .whitespacesAndNewlines)
        sendNotification(title: title, body: String(body), threadID: threadID)
    }

    static func clearNotifications() {
        UNUserNotificationCenter.current().removeAllDeliveredNotifications()
        UNUserNotificationCenter.current().removeAllPendingNotificationRequests()
    }
}
