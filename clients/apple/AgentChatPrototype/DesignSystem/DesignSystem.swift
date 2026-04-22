import SwiftUI

#if canImport(UIKit)
import UIKit
#endif

#if canImport(AppKit)
import AppKit
#endif

extension Color {
    init(_ token: ColorToken) {
        self.init(token: token)
    }

    init(token: ColorToken) {
        #if canImport(UIKit)
        switch token {
        case .blue:
            self = Color(uiColor: .systemBlue)
        case .purple:
            self = Color(uiColor: .systemPurple)
        case .green:
            self = Color(uiColor: .systemGreen)
        case .orange:
            self = Color(uiColor: .systemOrange)
        case .red:
            self = Color(uiColor: .systemRed)
        case .gray:
            self = Color(uiColor: .secondaryLabel)
        }
        #else
        switch token {
        case .blue:
            self = Color(nsColor: .controlAccentColor)
        case .purple:
            self = Color(nsColor: .systemPurple)
        case .green:
            self = Color(nsColor: .systemGreen)
        case .orange:
            self = Color(nsColor: .systemOrange)
        case .red:
            self = Color(nsColor: .systemRed)
        case .gray:
            self = Color.secondary
        }
        #endif
    }
}

enum AppColors {
    static var onlineStatus: Color {
        #if canImport(UIKit)
        Color(uiColor: .systemGreen)
        #else
        Color(nsColor: .systemGreen)
        #endif
    }

    static var unreadBadge: Color {
        #if canImport(UIKit)
        Color(uiColor: .systemRed)
        #else
        Color(nsColor: .systemRed)
        #endif
    }

    static var userBubble: Color {
        #if canImport(UIKit)
        Color(uiColor: .systemBlue)
        #else
        Color(nsColor: .controlAccentColor)
        #endif
    }
}

enum AppSpacing {
    static let xs: CGFloat = 4
    static let sm: CGFloat = 8
    static let md: CGFloat = 14
    static let lg: CGFloat = 20
    static let xl: CGFloat = 28
}

enum AppRadius {
    static let card: CGFloat = 16
    static let bubble: CGFloat = 18
    static let pill: CGFloat = 999
}

extension Color {
    static var appWindowBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .systemBackground)
        #else
        Color(nsColor: .windowBackgroundColor)
        #endif
    }

    static var appCanvasBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .systemGroupedBackground)
        #else
        Color(nsColor: .underPageBackgroundColor)
        #endif
    }

    static var appSidebarBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .secondarySystemGroupedBackground)
        #else
        Color(nsColor: .windowBackgroundColor)
        #endif
    }

    static var appCardBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .secondarySystemBackground)
        #else
        Color(nsColor: .controlBackgroundColor)
        #endif
    }

    static var appElevatedBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .secondarySystemGroupedBackground)
        #else
        Color(nsColor: .textBackgroundColor)
        #endif
    }

    static var appInputBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .tertiarySystemBackground)
        #else
        Color(nsColor: .textBackgroundColor)
        #endif
    }

    static var appHairline: Color {
        #if canImport(UIKit)
        Color(uiColor: .separator).opacity(0.18)
        #else
        Color(nsColor: .separatorColor).opacity(0.45)
        #endif
    }

    static var appSubtleFill: Color {
        #if canImport(UIKit)
        Color(uiColor: .tertiarySystemFill)
        #else
        Color(nsColor: .separatorColor).opacity(0.08)
        #endif
    }

    static var appSelectionFill: Color {
        #if canImport(UIKit)
        Color.accentColor.opacity(0.10)
        #else
        Color(nsColor: .selectedContentBackgroundColor).opacity(0.11)
        #endif
    }

    static var appSelectionStroke: Color {
        #if canImport(UIKit)
        Color.accentColor.opacity(0.16)
        #else
        Color(nsColor: .selectedContentBackgroundColor).opacity(0.22)
        #endif
    }
}

struct Theme {
    let colorScheme: ColorScheme

    var primaryText: Color {
        #if canImport(UIKit)
        Color(uiColor: .label)
        #else
        Color.primary
        #endif
    }

    var secondaryText: Color {
        #if canImport(UIKit)
        Color(uiColor: .secondaryLabel)
        #else
        Color.secondary
        #endif
    }

    var tertiaryText: Color {
        #if canImport(UIKit)
        Color(uiColor: .tertiaryLabel)
        #else
        Color.gray
        #endif
    }

    var background: Color {
        Color.appWindowBackground
    }

    var cardBackground: Color {
        Color.appElevatedBackground
    }

    var canvasBackground: Color {
        Color.appCanvasBackground
    }

    var inputBackground: Color {
        Color.appInputBackground
    }

    var separator: Color {
        Color.appHairline
    }

    var onlineStatus: Color {
        AppColors.onlineStatus
    }

    var accent: Color {
        Color.accentColor
    }

    var canvasTop: Color {
        Color.appCanvasBackground
    }

    var canvasBottom: Color {
        Color.appWindowBackground
    }

    var panel: Color {
        Color.appCardBackground
    }

    var paper: Color {
        Color.appElevatedBackground
    }

    var chip: Color {
        Color.appSubtleFill
    }

    var toolPanel: Color {
        Color.appCardBackground
    }

    var planPanel: Color {
        Color.appCardBackground
    }

    var stroke: Color {
        Color.appHairline
    }

    var ink: Color {
        primaryText
    }

    var mutedInk: Color {
        secondaryText
    }

    var subtleInk: Color {
        tertiaryText
    }

    var accentWarm: Color {
        #if canImport(UIKit)
        Color(uiColor: .systemOrange)
        #else
        Color(nsColor: .systemOrange)
        #endif
    }

    var planColor: Color {
        secondaryText
    }

    var userBubble: Color {
        AppColors.userBubble
    }
}

struct ThemedView<Content: View>: View {
    @Environment(\.colorScheme) var colorScheme
    let content: (Theme) -> Content

    var body: some View {
        content(Theme(colorScheme: colorScheme))
    }
}

struct CardSurface<Content: View>: View {
    var accent: ColorToken = .gray
    var isSelected: Bool = false
    var padding: CGFloat = AppSpacing.md
    @ViewBuilder var content: Content

    var body: some View {
        content
            .padding(padding)
            .background(
                RoundedRectangle(cornerRadius: AppRadius.card, style: .continuous)
                    .fill(Color.appCardBackground)
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppRadius.card, style: .continuous)
                    .fill(Color.appSelectionFill)
                    .opacity(isSelected ? 0.9 : 0)
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppRadius.card, style: .continuous)
                    .stroke(isSelected ? Color.appSelectionStroke : Color.appHairline, lineWidth: 1)
            )
    }
}

struct StatusBadge: View {
    var text: String
    var color: ColorToken

    var body: some View {
        Text(text)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(color == .gray ? .secondary : Color(color))
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(
                Capsule()
                    .fill(color == .gray ? Color.appSubtleFill : Color(color).opacity(0.08))
            )
            .overlay(
                Capsule()
                    .stroke(color == .gray ? Color.appHairline : Color(color).opacity(0.10), lineWidth: 1)
            )
    }
}

struct PillView: View {
    var text: String
    var color: ColorToken
    var isSelected: Bool = false

    var body: some View {
        Text(text)
            .font(.caption2.weight(.medium))
            .foregroundStyle(isSelected ? .primary : .secondary)
            .padding(.horizontal, 9)
            .padding(.vertical, 5)
            .background(
                Capsule()
                    .fill(isSelected ? Color.appSelectionFill : Color.appSubtleFill)
            )
            .overlay(
                Capsule()
                    .stroke(isSelected ? (color == .gray ? Color.appSelectionStroke : Color(color).opacity(0.12)) : Color.appHairline, lineWidth: 1)
            )
    }
}

struct AvatarView: View {
    var title: String
    var accent: ColorToken
    var size: CGFloat = 32

    var body: some View {
        ZStack {
            Circle()
                .fill(Color.appSubtleFill)
            Circle()
                .stroke(accent == .gray ? Color.appHairline : Color(accent).opacity(0.12), lineWidth: 1)
            Text(initials)
                .font(.system(size: size * 0.38, weight: .semibold))
                .foregroundStyle(.primary)
        }
        .frame(width: size, height: size)
    }

    private var initials: String {
        let pieces = title.split(separator: " ")
        let value = pieces.prefix(2).compactMap { $0.first }.map(String.init).joined()
        return value.isEmpty ? "A" : value.uppercased()
    }
}

enum PrototypeAvatarShape {
    case circle
    case roundedRect(cornerRadius: CGFloat)
}

struct PrototypeDefaultAvatarArtwork: View {
    let assetName: String
    let size: CGFloat
    let shape: PrototypeAvatarShape

    var body: some View {
        Group {
            switch shape {
            case .circle:
                Image(assetName)
                    .resizable()
                    .scaledToFill()
                    .frame(width: size, height: size)
                    .clipShape(Circle())
                    .overlay {
                        Circle()
                            .stroke(Color.black.opacity(0.06), lineWidth: 0.5)
                    }
            case .roundedRect(let cornerRadius):
                Image(assetName)
                    .resizable()
                    .scaledToFill()
                    .frame(width: size, height: size)
                    .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
                    .overlay {
                        RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                            .stroke(Color.black.opacity(0.06), lineWidth: 0.5)
                    }
            }
        }
    }
}

struct MetricLabel: View {
    var title: String
    var value: String

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(value)
                .font(.callout.monospacedDigit().weight(.semibold))
            Text(title)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }
}

struct EmptyStateView: View {
    var title: String
    var message: String
    var systemImage: String

    var body: some View {
        VStack(spacing: AppSpacing.md) {
            Image(systemName: systemImage)
                .font(.system(size: 34))
                .foregroundStyle(.secondary)
            Text(title)
                .font(.headline)
            Text(message)
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 360)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(AppSpacing.xl)
    }
}
