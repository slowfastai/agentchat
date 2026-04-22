import SwiftUI
import PhotosUI
#if canImport(UIKit)
import UIKit
#endif
#if canImport(AppKit)
import AppKit
#endif

struct AgentListView: View {
    @EnvironmentObject private var store: DemoStore
    @State private var searchText = ""
    @State private var selectedAgentID: UUID?
    @State private var showEditSheet = false
    @State private var showDeleteAlert = false
    @State private var agentToDelete: AgentProfile?
    @State private var editingAgent: AgentProfile?

    private var shortcutItems: [AgentShortcutItem] {
        [
            AgentShortcutItem(title: "New Agent", systemImage: "person.badge.plus", color: .orange),
            AgentShortcutItem(title: "Group Chats", systemImage: "person.3.fill", color: .green),
            AgentShortcutItem(title: "Labels", systemImage: "tag.fill", color: .blue),
            AgentShortcutItem(title: "Skill Channels", systemImage: "book.closed.fill", color: .purple)
        ]
    }

    private var filteredAgents: [AgentProfile] {
        guard !searchText.isEmpty else {
            return store.agents.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
        }

        return store.agents
            .filter { agent in
                let displayName = store.customName(for: agent.id.uuidString) ?? agent.name
                let searchable = [
                    displayName,
                    agent.shortDescription,
                    agent.capabilityTags.joined(separator: " ")
                ]
                .joined(separator: " ")
                .lowercased()

                return searchable.contains(searchText.lowercased())
            }
            .sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }

    private var groupedAgents: [AgentSection] {
        let grouped = Dictionary(grouping: filteredAgents) { agent in
            let displayName = store.customName(for: agent.id.uuidString) ?? agent.name
            return String(displayName.prefix(1)).uppercased()
        }

        return grouped.keys.sorted().map { key in
            AgentSection(
                title: key,
                agents: grouped[key]?.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending } ?? []
            )
        }
    }

    var body: some View {
        ZStack(alignment: .trailing) {
            List(selection: $selectedAgentID) {
                Section {
                    ForEach(shortcutItems) { item in
                        AgentShortcutRow(item: item)
                            .listRowInsets(EdgeInsets(top: 10, leading: 16, bottom: 10, trailing: 16))
                            .tag(nil as UUID?)
                    }
                }

                ForEach(groupedAgents) { section in
                    Section(section.title) {
                        ForEach(section.agents) { agent in
                            let agentID = agent.id.uuidString
                            AgentFriendRow(
                                agent: agent,
                                customName: store.customName(for: agentID),
                                avatarData: store.avatarData(for: agentID),
                                isConnecting: store.isConnecting(agentID: agentID),
                                isSelected: selectedAgentID == agent.id
                            )
                            .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
                            .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                                Button(role: .destructive) {
                                    agentToDelete = agent
                                    showDeleteAlert = true
                                } label: {
                                    Label("Delete", systemImage: "trash")
                                }

                                Button {
                                    editingAgent = agent
                                    showEditSheet = true
                                } label: {
                                    Label("Edit", systemImage: "pencil")
                                }
                                .tint(.blue)
                            }
                            .tag(agent.id)
                            .onTapGesture {
                                handleAgentTap(agent)
                            }
                        }
                    }
                }
            }
            .listStyle(.plain)
            .modifier(ActiveEditModeModifier())
            .scrollContentBackground(.hidden)
            .background(Color.appCanvasBackground)
            .navigationTitle("Agent Friends")
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button {
                        Task {
                            await store.refreshAgentsFromDaemon()
                        }
                    } label: {
                        Image(systemName: "arrow.clockwise")
                    }
                }
                
                if selectedAgentID != nil {
                    ToolbarItem(placement: .secondaryAction) {
                        Menu {
                            Button {
                                if let agentID = selectedAgentID,
                                   let agent = store.agents.first(where: { $0.id == agentID }) {
                                    editingAgent = agent
                                    showEditSheet = true
                                }
                            } label: {
                                Label("Edit Agent", systemImage: "pencil")
                            }
                            
                            Button(role: .destructive) {
                                if let agentID = selectedAgentID,
                                   let agent = store.agents.first(where: { $0.id == agentID }) {
                                    agentToDelete = agent
                                    showDeleteAlert = true
                                }
                            } label: {
                                Label("Delete Agent", systemImage: "trash")
                            }
                        } label: {
                            Image(systemName: "ellipsis.circle")
                        }
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
                    store.removeAgent(id: agent.id.uuidString)
                }
                agentToDelete = nil
            }
        } message: {
            if let agent = agentToDelete {
                let displayName = store.customName(for: agent.id.uuidString) ?? agent.name
                Text("Are you sure you want to delete \(displayName)? This action cannot be undone.")
            }
        }
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }

    private func handleAgentTap(_ agent: AgentProfile) {
        let agentID = agent.id.uuidString
        if store.isConnecting(agentID: agentID) {
            return
        }
        store.connectToAgent(id: agentID)
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
    let agents: [AgentProfile]

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
    let agent: AgentProfile
    let customName: String?
    let avatarData: Data?
    let isConnecting: Bool
    let isSelected: Bool

    var body: some View {
        HStack(spacing: 14) {
            ContactIconTile(
                title: customName ?? agent.name,
                accent: agent.accent,
                systemImage: systemImage(for: agent.kind),
                avatarAssetName: agent.resolvedDefaultAvatarAssetName,
                avatarData: avatarData
            )
            .overlay(
                RoundedRectangle(cornerRadius: 10)
                    .stroke(isSelected ? Color.blue : Color.clear, lineWidth: 2)
            )

            VStack(alignment: .leading, spacing: 3) {
                Text(customName ?? agent.name)
                    .font(.body)
                    .foregroundStyle(.primary)

                Text(subtitle)
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

    private var subtitle: String {
        if !agent.capabilityTags.isEmpty {
            return agent.capabilityTags.prefix(3).joined(separator: " · ")
        }
        return agent.shortDescription
    }

    private func systemImage(for kind: AgentKind) -> String {
        switch kind {
        case .claude:
            return "brain.head.profile"
        case .codex:
            return "curlybraces.square.fill"
        case .pi:
            return "sparkles"
        case .opencode:
            return "terminal.fill"
        case .human:
            return "person.fill"
        }
    }
}

private struct ContactIconTile: View {
    let title: String
    let accent: ColorToken
    let systemImage: String
    var avatarAssetName: String? = nil
    var avatarData: Data?

    var body: some View {
        if let data = avatarData {
            PrototypeAvatarDataImage(data: data, size: 42, cornerRadius: 10)
        } else if let avatarAssetName {
            PrototypeDefaultAvatarArtwork(
                assetName: avatarAssetName,
                size: 42,
                shape: .roundedRect(cornerRadius: 10)
            )
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

private struct ActiveEditModeModifier: ViewModifier {
    func body(content: Content) -> some View {
        #if os(macOS)
        content
        #else
        content.environment(\.editMode, .constant(.active))
        #endif
    }
}

private struct PrototypeAvatarDataImage: View {
    let data: Data
    let size: CGFloat
    let cornerRadius: CGFloat

    var body: some View {
        Group {
            #if canImport(UIKit)
            if let image = UIImage(data: data) {
                Image(uiImage: image)
                    .resizable()
                    .scaledToFill()
            } else {
                Color.clear
            }
            #elseif canImport(AppKit)
            if let image = NSImage(data: data) {
                Image(nsImage: image)
                    .resizable()
                    .scaledToFill()
            } else {
                Color.clear
            }
            #else
            Color.clear
            #endif
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
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
    @EnvironmentObject private var store: DemoStore
    @Environment(\.dismiss) private var dismiss

    let agent: AgentProfile

    @State private var editedName: String = ""
    @State private var selectedPhotoItem: PhotosPickerItem?
    @State private var selectedImageData: Data?
    @State private var showError = false

    private var displayName: String {
        store.customName(for: agent.id.uuidString) ?? agent.name
    }

    private var canSave: Bool {
        !editedName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        VStack(spacing: 0) {
            VStack(spacing: 22) {
                VStack(spacing: 14) {
                    PhotosPicker(selection: $selectedPhotoItem, matching: .images) {
                        AgentEditableAvatarPreview(
                            agent: agent,
                            displayName: editedName.isEmpty ? displayName : editedName,
                            avatarData: selectedImageData
                        )
                    }
                    .buttonStyle(.plain)
                    .onChange(of: selectedPhotoItem) { _, newValue in
                        Task {
                            if let data = try? await newValue?.loadTransferable(type: Data.self) {
                                selectedImageData = processImage(data)
                            }
                        }
                    }

                    VStack(spacing: 8) {
                        Text("Edit Agent")
                            .font(.title2.weight(.semibold))

                        Text(displayName)
                            .font(.callout)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }

                    HStack(spacing: 10) {
                        PhotosPicker(selection: $selectedPhotoItem, matching: .images) {
                            Label("Change Photo", systemImage: "photo")
                        }
                        .buttonStyle(.borderless)

                        if selectedImageData != nil || store.avatarData(for: agent.id.uuidString) != nil {
                            Button(role: .destructive) {
                                selectedImageData = nil
                                selectedPhotoItem = nil
                            } label: {
                                Label("Remove", systemImage: "trash")
                            }
                            .buttonStyle(.borderless)
                        }
                    }
                    .font(.callout)
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text("Name")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)

                    TextField("Agent name", text: $editedName)
                        .textFieldStyle(.roundedBorder)
                        .font(.body)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.horizontal, 30)
            .padding(.top, 30)
            .padding(.bottom, 26)

            Divider()

            HStack(spacing: 10) {
                Spacer()

                Button("Cancel") {
                    dismiss()
                }
                .keyboardShortcut(.cancelAction)

                Button("Save") {
                    saveChanges()
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(!canSave)
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 16)
            .background(Color.appWindowBackground)
        }
        .frame(maxWidth: 440)
        .background(Color.appCanvasBackground)
        .onAppear {
            editedName = displayName
            selectedImageData = store.avatarData(for: agent.id.uuidString)
        }
        .alert("Error", isPresented: $showError) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("Failed to save changes. Please try again.")
        }
    }

    private func processImage(_ data: Data) -> Data? {
        #if canImport(UIKit)
        guard let uiImage = UIImage(data: data) else { return nil }

        let maxSize: CGFloat = 200
        let scale = min(maxSize / uiImage.size.width, maxSize / uiImage.size.height)
        let newSize = CGSize(width: uiImage.size.width * scale, height: uiImage.size.height * scale)

        UIGraphicsBeginImageContextWithOptions(newSize, false, 1.0)
        uiImage.draw(in: CGRect(origin: .zero, size: newSize))
        let resizedImage = UIGraphicsGetImageFromCurrentImageContext()
        UIGraphicsEndImageContext()

        return resizedImage?.jpegData(compressionQuality: 0.8)
        #elseif canImport(AppKit)
        guard let image = NSImage(data: data) else { return nil }

        let maxSize: CGFloat = 200
        let scale = min(maxSize / image.size.width, maxSize / image.size.height)
        let newSize = CGSize(width: image.size.width * scale, height: image.size.height * scale)
        let resizedImage = NSImage(size: newSize)
        resizedImage.lockFocus()
        image.draw(in: CGRect(origin: .zero, size: newSize))
        resizedImage.unlockFocus()

        guard
            let tiffData = resizedImage.tiffRepresentation,
            let bitmap = NSBitmapImageRep(data: tiffData)
        else {
            return data
        }

        return bitmap.representation(using: .jpeg, properties: [.compressionFactor: 0.8])
        #else
        return data
        #endif
    }

    private func saveChanges() {
        let trimmedName = editedName.trimmingCharacters(in: .whitespacesAndNewlines)
        let nameToSave = trimmedName.isEmpty ? nil : trimmedName

        store.updateAgent(
            id: agent.id.uuidString,
            name: nameToSave,
            avatarData: selectedImageData
        )
        dismiss()
    }
}

private struct AgentEditableAvatarPreview: View {
    let agent: AgentProfile
    let displayName: String
    let avatarData: Data?

    var body: some View {
        ZStack(alignment: .bottomTrailing) {
            Group {
                if let avatarData {
                    PrototypeAvatarDataImage(data: avatarData, size: 96, cornerRadius: 48)
                        .clipShape(Circle())
                } else if let assetName = agent.resolvedDefaultAvatarAssetName {
                    PrototypeDefaultAvatarArtwork(
                        assetName: assetName,
                        size: 96,
                        shape: .circle
                    )
                } else {
                    AvatarView(title: displayName, accent: agent.accent, size: 96)
                }
            }
            .overlay {
                Circle()
                    .stroke(Color.black.opacity(0.08), lineWidth: 1)
            }

            Circle()
                .fill(Color.appElevatedBackground)
                .frame(width: 28, height: 28)
                .overlay {
                    Image(systemName: "camera.fill")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(.secondary)
                }
                .overlay {
                    Circle()
                        .stroke(Color.appHairline, lineWidth: 1)
                }
                .shadow(color: .black.opacity(0.10), radius: 6, y: 2)
        }
        .frame(width: 104, height: 104)
    }
}
