import SwiftUI

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

enum AgentAvatarShape {
    case circle
    case roundedRect(cornerRadius: CGFloat)
}

struct AgentDefaultAvatarArtwork: View {
    let assetName: String
    let size: CGFloat
    let shape: AgentAvatarShape

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