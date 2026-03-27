import SwiftUI
#if os(iOS)
import UIKit
#endif

enum AgentAvatarPalette {
    static func tintColor(named tintName: String) -> Color {
        switch tintName {
        case "purple": return .purple
        case "green": return .green
        case "orange": return .orange
        case "blue": return .blue
        case "gray": return .gray
        case "red": return .red
        default: return .indigo
        }
    }
}

struct AgentAvatarView: View {
    let agent: DaemonAgentSummary
    var size: CGFloat = 40

    var body: some View {
        ZStack {
            if let imageData = agent.avatarImageData,
               let uiImage = UIImage(data: imageData) {
                Image(uiImage: uiImage)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: size, height: size)
                    .clipShape(Circle())
            } else {
                ZStack {
                    Circle()
                        .fill(AgentAvatarPalette.tintColor(named: agent.tintName).opacity(0.14))

                    Image(systemName: agent.symbolName)
                        .font(.system(size: size * 0.45, weight: .semibold))
                        .foregroundStyle(AgentAvatarPalette.tintColor(named: agent.tintName))
                }
                .frame(width: size, height: size)
            }
        }
    }
}
