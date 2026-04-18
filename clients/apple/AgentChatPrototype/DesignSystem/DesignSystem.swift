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
        switch token {
        case .blue:
            self = .blue
        case .purple:
            self = .purple
        case .green:
            self = .green
        case .orange:
            self = .orange
        case .red:
            self = .red
        case .gray:
            self = .gray
        }
    }
}

enum AppColors {
    static var onlineStatus: Color {
        Color(red: 0.3, green: 0.85, blue: 0.5)
    }

    static var unreadBadge: Color {
        Color(red: 1.0, green: 0.35, blue: 0.35)
    }

    static var userBubble: Color {
        Color(red: 0.15, green: 0.45, blue: 0.85)
    }
}

enum AppSpacing {
    static let xs: CGFloat = 6
    static let sm: CGFloat = 10
    static let md: CGFloat = 16
    static let lg: CGFloat = 24
    static let xl: CGFloat = 32
}

enum AppRadius {
    static let card: CGFloat = 18
    static let bubble: CGFloat = 18
    static let pill: CGFloat = 999
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
        #if canImport(UIKit)
        Color(uiColor: .systemBackground)
        #else
        Color(nsColor: .windowBackgroundColor)
        #endif
    }

    var cardBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .secondarySystemBackground)
        #else
        Color(nsColor: .controlBackgroundColor)
        #endif
    }

    var canvasBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .systemGroupedBackground)
        #else
        Color(nsColor: .windowBackgroundColor)
        #endif
    }

    var inputBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .tertiarySystemBackground)
        #else
        Color(nsColor: .textBackgroundColor)
        #endif
    }

    var separator: Color {
        #if canImport(UIKit)
        Color(uiColor: .separator)
        #else
        Color(nsColor: .separatorColor)
        #endif
    }

    var onlineStatus: Color {
        Color(red: 0.3, green: 0.85, blue: 0.5)
    }

    var accent: Color {
        Color.accentColor
    }

    var canvasTop: Color {
        colorScheme == .dark
            ? Color(red: 0.11, green: 0.11, blue: 0.13)
            : Color(red: 0.973, green: 0.957, blue: 0.929)
    }

    var canvasBottom: Color {
        colorScheme == .dark
            ? Color(red: 0.10, green: 0.10, blue: 0.12)
            : Color(red: 0.948, green: 0.928, blue: 0.895)
    }

    var panel: Color {
        colorScheme == .dark
            ? Color(red: 0.16, green: 0.16, blue: 0.18)
            : Color(red: 0.981, green: 0.971, blue: 0.949)
    }

    var paper: Color {
        colorScheme == .dark
            ? Color(red: 0.20, green: 0.20, blue: 0.22)
            : Color(red: 0.993, green: 0.988, blue: 0.976)
    }

    var chip: Color {
        colorScheme == .dark
            ? Color(red: 0.26, green: 0.26, blue: 0.28)
            : Color(red: 0.936, green: 0.918, blue: 0.885)
    }

    var toolPanel: Color {
        colorScheme == .dark
            ? Color(red: 0.18, green: 0.17, blue: 0.19)
            : Color(red: 0.957, green: 0.936, blue: 0.892)
    }

    var planPanel: Color {
        colorScheme == .dark
            ? Color(red: 0.15, green: 0.15, blue: 0.17)
            : Color(red: 0.933, green: 0.918, blue: 0.900)
    }

    var stroke: Color {
        colorScheme == .dark
            ? Color.white.opacity(0.08)
            : Color.black.opacity(0.075)
    }

    var ink: Color {
        colorScheme == .dark
            ? Color(red: 0.90, green: 0.90, blue: 0.92)
            : Color(red: 0.200, green: 0.188, blue: 0.173)
    }

    var mutedInk: Color {
        colorScheme == .dark
            ? Color(red: 0.60, green: 0.60, blue: 0.62)
            : Color(red: 0.420, green: 0.392, blue: 0.357)
    }

    var subtleInk: Color {
        colorScheme == .dark
            ? Color(red: 0.50, green: 0.50, blue: 0.52)
            : Color(red: 0.550, green: 0.514, blue: 0.470)
    }

    var accentWarm: Color {
        colorScheme == .dark
            ? Color(red: 0.80, green: 0.60, blue: 0.35)
            : Color(red: 0.694, green: 0.533, blue: 0.333)
    }

    var planColor: Color {
        colorScheme == .dark
            ? Color(red: 0.60, green: 0.52, blue: 0.48)
            : Color(red: 0.463, green: 0.392, blue: 0.361)
    }

    var userBubble: Color {
        colorScheme == .dark
            ? Color(red: 0.35, green: 0.32, blue: 0.30)
            : Color(red: 0.274, green: 0.251, blue: 0.228)
    }
}

struct ThemedView<Content: View>: View {
    @Environment(\.colorScheme) var colorScheme
    let content: (Theme) -> Content

    var body: some View {
        content(Theme(colorScheme: colorScheme))
    }
}

extension Color {
    static var appCardBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .secondarySystemBackground)
        #else
        Color(nsColor: .controlBackgroundColor)
        #endif
    }

    static var appCanvasBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .systemGroupedBackground)
        #else
        Color(nsColor: .windowBackgroundColor)
        #endif
    }

    static var appInputBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .tertiarySystemBackground)
        #else
        Color(nsColor: .textBackgroundColor)
        #endif
    }
}

struct CardSurface<Content: View>: View {
    var accent: ColorToken = .gray
    var isSelected: Bool = false
    @ViewBuilder var content: Content

    var body: some View {
        content
            .padding(AppSpacing.md)
            .background(
                RoundedRectangle(cornerRadius: AppRadius.card, style: .continuous)
                    .fill(Color.appCardBackground)
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppRadius.card, style: .continuous)
                    .stroke(isSelected ? Color(accent).opacity(0.8) : Color.primary.opacity(0.08), lineWidth: isSelected ? 1.5 : 1)
            )
    }
}

struct StatusBadge: View {
    var text: String
    var color: ColorToken

    var body: some View {
        Text(text)
            .font(.caption.weight(.semibold))
            .foregroundStyle(Color(color))
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(Color(color).opacity(0.12), in: Capsule())
    }
}

struct PillView: View {
    var text: String
    var color: ColorToken
    var isSelected: Bool = false

    var body: some View {
        Text(text)
            .font(.caption.weight(.medium))
            .foregroundStyle(isSelected ? .white : Color(color))
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(
                Capsule()
                    .fill(isSelected ? Color(color) : Color(color).opacity(0.12))
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
                .fill(Color(accent).opacity(0.14))
            Text(initials)
                .font(.system(size: size * 0.38, weight: .semibold))
                .foregroundStyle(Color(accent))
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
                .font(.subheadline.weight(.semibold))
            Text(title)
                .font(.caption)
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
                .font(.system(size: 42))
                .foregroundStyle(.secondary)
            Text(title)
                .font(.title3.weight(.semibold))
            Text(message)
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 360)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(AppSpacing.xl)
    }
}
