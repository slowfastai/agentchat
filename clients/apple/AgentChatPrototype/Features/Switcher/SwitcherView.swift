import SwiftUI

struct SwitcherView: View {
    @Binding var selectedMode: SwitcherMode

    var body: some View {
        CardSurface(accent: .purple) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                Text("Switcher")
                    .font(.headline)

                Text("Swap between list, grid, and focus layouts for the current workspace context.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                Picker("View", selection: $selectedMode) {
                    ForEach(SwitcherMode.allCases) { mode in
                        Label(mode.title, systemImage: mode.systemImage)
                            .tag(mode)
                    }
                }
                .pickerStyle(.segmented)
            }
        }
    }
}

private extension SwitcherMode {
    var systemImage: String {
        switch self {
        case .list:
            return "list.bullet"
        case .grid:
            return "square.grid.2x2"
        case .focus:
            return "viewfinder"
        }
    }
}

#Preview {
    SwitcherView(selectedMode: .constant(.list))
        .padding()
}
