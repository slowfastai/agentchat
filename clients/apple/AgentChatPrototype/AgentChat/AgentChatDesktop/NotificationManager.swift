import Foundation
import UserNotifications
import AppKit

enum NotificationHelper {
    static var isAuthorized = false
    private static var deliveredEventIDs = Set<String>()

    static func configure() {
        UNUserNotificationCenter.current().delegate = NotificationDelegate.shared
    }

    static func requestAuthorization() async {
        configure()
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

    static func sendNotification(
        title: String,
        body: String,
        threadID: String? = nil,
        eventID: String? = nil
    ) {
        guard isAuthorized else { return }
        if let eventID {
            guard deliveredEventIDs.insert(eventID).inserted else { return }
        }

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
                identifier: eventID.map { "agent-response-\($0)" } ?? UUID().uuidString,
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

    static func sendAgentResponseNotification(
        agentName: String,
        message: String,
        threadID: String,
        eventID: String? = nil
    ) {
        let title = agentName.isEmpty ? "AgentChat" : agentName
        let preview = message.prefix(100).trimmingCharacters(in: .whitespacesAndNewlines)
        let body = preview.isEmpty ? "Response finished." : String(preview)
        sendNotification(title: title, body: body, threadID: threadID, eventID: eventID)
    }

    static func clearNotifications() {
        UNUserNotificationCenter.current().removeAllDeliveredNotifications()
        UNUserNotificationCenter.current().removeAllPendingNotificationRequests()
    }
}

private final class NotificationDelegate: NSObject, UNUserNotificationCenterDelegate {
    static let shared = NotificationDelegate()

    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .sound]
    }
}
