import SwiftUI
#if os(iOS)
import UIKit
#elseif os(macOS)
import AppKit
#endif

extension ColorToken {
    var color: Color {
        switch self {
        case .blue: return .blue
        case .purple: return .purple
        case .green: return .green
        case .orange: return .orange
        case .red: return .red
        case .gray: return .gray
        }
    }
}

extension Color {
    static var appCardBackground: Color {
        #if os(iOS)
        return Color(uiColor: .secondarySystemBackground)
        #elseif os(macOS)
        return Color(nsColor: .windowBackgroundColor)
        #else
        return Color.gray.opacity(0.08)
        #endif
    }

    static var appCanvasBackground: Color {
        #if os(iOS)
        return Color(uiColor: .systemGroupedBackground)
        #elseif os(macOS)
        return Color(nsColor: .controlBackgroundColor)
        #else
        return Color.gray.opacity(0.05)
        #endif
    }

    static var appInputBackground: Color {
        #if os(iOS)
        return Color(uiColor: .tertiarySystemBackground)
        #elseif os(macOS)
        return Color(nsColor: .textBackgroundColor)
        #else
        return Color.gray.opacity(0.06)
        #endif
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
                    .stroke(isSelected ? accent.color.opacity(0.8) : Color.primary.opacity(0.08), lineWidth: isSelected ? 1.5 : 1)
            )
    }
}

struct StatusBadge: View {
    var text: String
    var color: ColorToken

    var body: some View {
        Text(text)
            .font(.caption.weight(.semibold))
            .foregroundStyle(color.color)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(color.color.opacity(0.12), in: Capsule())
    }
}

struct PillView: View {
    var text: String
    var color: ColorToken
    var isSelected: Bool = false

    var body: some View {
        Text(text)
            .font(.caption.weight(.medium))
            .foregroundStyle(isSelected ? .white : color.color)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(
                Capsule()
                    .fill(isSelected ? color.color : color.color.opacity(0.12))
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
                .fill(accent.color.opacity(0.14))
            Text(initials)
                .font(.system(size: size * 0.38, weight: .semibold))
                .foregroundStyle(accent.color)
        }
        .frame(width: size, height: size)
    }

    private var initials: String {
        let pieces = title.split(separator: " ")
        let value = pieces.prefix(2).compactMap { $0.first }.map(String.init).joined()
        return value.isEmpty ? "A" : value.uppercased()
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
