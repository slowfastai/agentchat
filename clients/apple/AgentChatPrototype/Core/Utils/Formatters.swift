import Foundation

enum AppFormatters {
    static let relative: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .short
        return formatter
    }()

    static let duration: DateComponentsFormatter = {
        let formatter = DateComponentsFormatter()
        formatter.allowedUnits = [.hour, .minute]
        formatter.unitsStyle = .abbreviated
        formatter.maximumUnitCount = 2
        return formatter
    }()

    static let time: DateFormatter = {
        let formatter = DateFormatter()
        formatter.timeStyle = .short
        return formatter
    }()

    static func relativeString(from date: Date) -> String {
        relative.localizedString(for: date, relativeTo: Date())
    }

    static func durationString(seconds: Int) -> String {
        duration.string(from: TimeInterval(seconds)) ?? "0m"
    }

    static func timeString(from date: Date) -> String {
        time.string(from: date)
    }
}
