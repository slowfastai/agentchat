import SwiftUI
import PhotosUI
#if canImport(UIKit)
import UIKit
#endif
#if canImport(AppKit)
import AppKit
#endif

enum AppColors {
    static var onlineStatus: Color {
        Color(red: 0.3, green: 0.85, blue: 0.5)
    }

    static var unreadBadge: Color {
        Color(red: 1.0, green: 0.35, blue: 0.35)
    }
}

struct AgentListView: View {
    @EnvironmentObject private var store: DaemonChatStore
    @State private var searchText = ""
    @State private var selectedAgentID: String?
    @State private var showEditSheet = false
    @State private var showDeleteAlert = false
    @State private var agentToDelete: DaemonAgentSummary?
    @State private var editingAgent: DaemonAgentSummary?

    private var shortcutItems: [AgentShortcutItem] {
        [
            AgentShortcutItem(title: "New Agent", systemImage: "person.badge.plus", color: .orange),
            AgentShortcutItem(title: "Group Chats", systemImage: "person.3.fill", color: .green),
            AgentShortcutItem(title: "Labels", systemImage: "tag.fill", color: .blue),
            AgentShortcutItem(title: "Skill Channels", systemImage: "book.closed.fill", color: .purple)
        ]
    }

    private var filteredAgents: [DaemonAgentSummary] {
        guard !searchText.isEmpty else {
            return store.agents.sorted { $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending }
        }

        return store.agents
            .filter { agent in
                let displayName = store.customName(for: agent.agentID) ?? agent.displayName
                let searchable = [
                    displayName,
                    agent.capabilitySummary,
                    agent.kindTitle
                ]
                .joined(separator: " ")
                .lowercased()

                return searchable.contains(searchText.lowercased())
            }
            .sorted { $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending }
    }

    private var groupedAgents: [AgentSection] {
        let grouped = Dictionary(grouping: filteredAgents) { agent in
            let displayName = store.customName(for: agent.agentID) ?? agent.displayName
            return String(displayName.prefix(1)).uppercased()
        }

        return grouped.keys.sorted().map { key in
            AgentSection(
                title: key,
                agents: grouped[key]?.sorted { $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending } ?? []
            )
        }
    }

    var body: some View {
        ZStack(alignment: .trailing) {
            List {
                Section {
                    ForEach(shortcutItems) { item in
                        AgentShortcutRow(item: item)
                            .listRowInsets(EdgeInsets(top: 10, leading: 16, bottom: 10, trailing: 16))
                    }
                }

                ForEach(groupedAgents) { section in
                    Section(section.title) {
                        ForEach(section.agents) { agent in
                            AgentFriendRow(
                                agent: agent,
                                customName: store.customName(for: agent.agentID),
                                avatarData: store.avatarData(for: agent.agentID),
                                isConnecting: store.isConnecting(agentID: agent.agentID)
                            )
                            .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
                            .onTapGesture {
                                handleAgentTap(agent)
                            }
                            .contextMenu {
                                Button {
                                    editingAgent = agent
                                    showEditSheet = true
                                } label: {
                                    Label("Edit Agent", systemImage: "pencil")
                                }

                                Button(role: .destructive) {
                                    agentToDelete = agent
                                    showDeleteAlert = true
                                } label: {
                                    Label("Delete Agent", systemImage: "trash")
                                }
                            }
                        }
                    }
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .background(Color.appCanvasBackground)
            .searchable(text: $searchText, prompt: "Search agents")
            .navigationTitle("Agent Friends")
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button {
                    } label: {
                        Image(systemName: "person.badge.plus")
                    }
                }
            }

            if !groupedAgents.isEmpty {
                AgentSectionIndexOverlay(letters: groupedAgents.map(\.title))
                    .padding(.trailing, 6)
            }
        }
        .sheet(isPresented: $showEditSheet) {
            if let agent = editingAgent {
                EditAgentSheet(agent: agent)
            }
        }
        .alert("Delete Agent", isPresented: $showDeleteAlert) {
            Button("Cancel", role: .cancel) {
                agentToDelete = nil
            }
            Button("Delete", role: .destructive) {
                if let agent = agentToDelete {
                    store.removeAgent(id: agent.agentID)
                }
                agentToDelete = nil
            }
        } message: {
            if let agent = agentToDelete {
                let displayName = store.customName(for: agent.agentID) ?? agent.displayName
                Text("Are you sure you want to delete \(displayName)? This action cannot be undone.")
            }
        }
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }

    private func handleAgentTap(_ agent: DaemonAgentSummary) {
        if store.isConnecting(agentID: agent.agentID) {
            return
        }

        if store.hasConfiguredDaemonURL {
            store.connectToAgent(id: agent.agentID)
        }
    }
}

private struct AgentShortcutItem: Identifiable {
    let id = UUID()
    let title: String
    let systemImage: String
    let color: ColorToken
}

private struct AgentSection: Identifiable {
    let title: String
    let agents: [DaemonAgentSummary]

    var id: String { title }
}

private struct AgentShortcutRow: View {
    let item: AgentShortcutItem

    var body: some View {
        HStack(spacing: 14) {
            ContactIconTile(title: item.title, accent: item.color, systemImage: item.systemImage)

            Text(item.title)
                .font(.body)
                .foregroundStyle(.primary)

            Spacer()
        }
        .contentShape(Rectangle())
    }
}

private struct AgentFriendRow: View {
    let agent: DaemonAgentSummary
    let customName: String?
    let avatarData: Data?
    let isConnecting: Bool

    var body: some View {
        HStack(spacing: 14) {
            ContactIconTile(
                title: customName ?? agent.displayName,
                accent: ColorToken(rawValue: agent.tintName) ?? .blue,
                systemImage: agent.symbolName,
                avatarData: avatarData
            )

            VStack(alignment: .leading, spacing: 3) {
                Text(customName ?? agent.displayName)
                    .font(.body)
                    .foregroundStyle(.primary)

                Text(agent.capabilitySummary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer()

            if isConnecting {
                HStack(spacing: 6) {
                    Circle()
                        .fill(Color.orange)
                        .frame(width: 8, height: 8)
                    Text("Connecting...")
                        .font(.caption2)
                        .foregroundStyle(.orange)
                }
            } else if agent.isOnline {
                HStack(spacing: 6) {
                    Circle()
                        .fill(AppColors.onlineStatus)
                        .frame(width: 8, height: 8)
                    Text("online")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .contentShape(Rectangle())
    }
}

private struct ContactIconTile: View {
    let title: String
    let accent: ColorToken
    let systemImage: String
    var avatarData: Data?

    var body: some View {
        if let data = avatarData, let uiImage = UIImage(data: data) {
            Image(uiImage: uiImage)
                .resizable()
                .scaledToFill()
                .frame(width: 42, height: 42)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        } else {
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(accent.color)
                .frame(width: 42, height: 42)
                .overlay {
                    Image(systemName: systemImage)
                        .font(.system(size: 19, weight: .semibold))
                        .foregroundStyle(.white)
                }
        }
    }
}

private struct AgentSectionIndexOverlay: View {
    let letters: [String]

    var body: some View {
        VStack(spacing: 3) {
            Image(systemName: "magnifyingglass")
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.bottom, 2)

            ForEach(letters, id: \.self) { letter in
                Text(letter)
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 8)
        .frame(width: 18)
        .background(Color.clear)
    }
}

struct EditAgentSheet: View {
    @EnvironmentObject private var store: DaemonChatStore
    @Environment(\.dismiss) private var dismiss

    let agent: DaemonAgentSummary

    @State private var editedName: String = ""
    @State private var selectedPhotoItem: PhotosPickerItem?
    @State private var selectedImageData: Data?
    @State private var showError = false

    var body: some View {
        NavigationStack {
            Form {
                Section("Agent Info") {
                    TextField("Name", text: $editedName)

                    if let data = selectedImageData, let uiImage = UIImage(data: data) {
                        HStack {
                            Spacer()
                            Image(uiImage: uiImage)
                                .resizable()
                                .scaledToFill()
                                .frame(width: 80, height: 80)
                                .clipShape(RoundedRectangle(cornerRadius: 16))
                            Spacer()
                        }
                    }
                }

                Section("Avatar") {
                    PhotosPicker(selection: $selectedPhotoItem, matching: .images) {
                        Label("Choose Photo", systemImage: "photo")
                    }
                    .onChange(of: selectedPhotoItem) { _, newValue in
                        Task {
                            if let data = try? await newValue?.loadTransferable(type: Data.self) {
                                selectedImageData = processImage(data)
                            }
                        }
                    }

                    if selectedImageData != nil || store.avatarData(for: agent.agentID) != nil {
                        Button(role: .destructive) {
                            selectedImageData = nil
                            selectedPhotoItem = nil
                        } label: {
                            Label("Remove Photo", systemImage: "trash")
                        }
                    }
                }
            }
            .navigationTitle("Edit Agent")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        saveChanges()
                    }
                    .disabled(editedName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && selectedImageData == nil)
                }
            }
            .onAppear {
                editedName = store.customName(for: agent.agentID) ?? agent.displayName
                selectedImageData = store.avatarData(for: agent.agentID)
            }
            .alert("Error", isPresented: $showError) {
                Button("OK", role: .cancel) {}
            } message: {
                Text("Failed to save changes. Please try again.")
            }
        }
    }

    private func processImage(_ data: Data) -> Data? {
        guard let uiImage = UIImage(data: data) else { return nil }

        let maxSize: CGFloat = 200
        let scale = min(maxSize / uiImage.size.width, maxSize / uiImage.size.height)
        let newSize = CGSize(width: uiImage.size.width * scale, height: uiImage.size.height * scale)

        UIGraphicsBeginImageContextWithOptions(newSize, false, 1.0)
        uiImage.draw(in: CGRect(origin: .zero, size: newSize))
        let resizedImage = UIGraphicsGetImageFromCurrentImageContext()
        UIGraphicsEndImageContext()

        return resizedImage?.jpegData(compressionQuality: 0.8)
    }

    private func saveChanges() {
        let trimmedName = editedName.trimmingCharacters(in: .whitespacesAndNewlines)
        let nameToSave = trimmedName.isEmpty ? nil : trimmedName

        store.updateAgent(
            id: agent.agentID,
            name: nameToSave,
            avatarData: selectedImageData
        )
        dismiss()
    }
}
