import SwiftUI
import UniformTypeIdentifiers

struct AgentEditSheet: View {
    let agent: DaemonAgentSummary
    let initialSettings: AgentLocalSettings
    let onSave: (String?, Data?, AgentLocalSettings?) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var displayName: String
    @State private var selectedImage: NSImage?
    @State private var showImagePicker = false
    @State private var showRemovePhotoAlert = false
    @State private var model: String
    @State private var systemPrompt: String
    @State private var codexReasoningEffort: AgentReasoningEffort?
    @State private var codexApprovalPolicy: AgentApprovalPolicy?
    @State private var claudeThinkingMode: AgentThinkingMode?
    @State private var claudeFallbackModel: String
    @State private var piProfile: String
    @State private var piQuietStartupChoice: QuietStartupChoice
    @State private var piResponseStyle: PiResponseStyle?
    @State private var openCodeProvider: String
    @State private var openCodeExecutionMode: String

    private enum QuietStartupChoice: String, CaseIterable, Identifiable {
        case useDaemonDefault
        case enabled
        case disabled

        var id: String { rawValue }

        var title: String {
            switch self {
            case .useDaemonDefault:
                return "Daemon Default"
            case .enabled:
                return "Enabled"
            case .disabled:
                return "Disabled"
            }
        }

        var value: Bool? {
            switch self {
            case .useDaemonDefault:
                return nil
            case .enabled:
                return true
            case .disabled:
                return false
            }
        }

        static func from(_ value: Bool?) -> Self {
            switch value {
            case .some(true):
                return .enabled
            case .some(false):
                return .disabled
            case .none:
                return .useDaemonDefault
            }
        }
    }

    init(
        agent: DaemonAgentSummary,
        initialSettings: AgentLocalSettings = AgentLocalSettings(),
        onSave: @escaping (String?, Data?, AgentLocalSettings?) -> Void
    ) {
        self.agent = agent
        self.initialSettings = initialSettings.normalized
        self.onSave = onSave
        _displayName = State(initialValue: agent.customDisplayName ?? "")
        if let imageData = agent.avatarImageData {
            _selectedImage = State(initialValue: NSImage(data: imageData))
        }
        _model = State(initialValue: initialSettings.model ?? "")
        _systemPrompt = State(initialValue: initialSettings.systemPrompt ?? "")
        _codexReasoningEffort = State(initialValue: initialSettings.codexReasoningEffort)
        _codexApprovalPolicy = State(initialValue: initialSettings.codexApprovalPolicy)
        _claudeThinkingMode = State(initialValue: initialSettings.claudeThinkingMode)
        _claudeFallbackModel = State(initialValue: initialSettings.claudeFallbackModel ?? "")
        _piProfile = State(initialValue: initialSettings.piProfile ?? "")
        _piQuietStartupChoice = State(initialValue: QuietStartupChoice.from(initialSettings.piQuietStartup))
        _piResponseStyle = State(initialValue: initialSettings.piResponseStyle)
        _openCodeProvider = State(initialValue: initialSettings.openCodeProvider ?? "")
        _openCodeExecutionMode = State(initialValue: initialSettings.openCodeExecutionMode ?? "")
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            Form {
                Section {
                    avatarSection
                }

                Section("Identity") {
                    TextField("Enter a custom name", text: $displayName)
                }

                Section("Runtime") {
                    TextField(modelPlaceholder, text: $model)

                    TextField("Optional instructions or persona notes", text: $systemPrompt, axis: .vertical)
                        .lineLimit(3...6)
                }

                if agent.family == .codex {
                    Section("Codex") {
                        Picker("Reasoning", selection: $codexReasoningEffort) {
                            Text("Daemon Default").tag(nil as AgentReasoningEffort?)
                            ForEach(AgentReasoningEffort.allCases) { effort in
                                Text(effort.title).tag(effort as AgentReasoningEffort?)
                            }
                        }

                        Picker("Approvals", selection: $codexApprovalPolicy) {
                            Text("Daemon Default").tag(nil as AgentApprovalPolicy?)
                            ForEach(AgentApprovalPolicy.allCases) { policy in
                                Text(policy.title).tag(policy as AgentApprovalPolicy?)
                            }
                        }
                    }
                }

                if agent.family == .claude {
                    Section("Claude Code") {
                        Picker("Thinking", selection: $claudeThinkingMode) {
                            Text("Daemon Default").tag(nil as AgentThinkingMode?)
                            ForEach(AgentThinkingMode.allCases) { mode in
                                Text(mode.title).tag(mode as AgentThinkingMode?)
                            }
                        }

                        TextField("Fallback model", text: $claudeFallbackModel)
                    }
                }

                if agent.family == .pi {
                    Section("Pi") {
                        TextField("Profile or config preset", text: $piProfile)

                        Picker("Quiet startup", selection: $piQuietStartupChoice) {
                            ForEach(QuietStartupChoice.allCases) { choice in
                                Text(choice.title).tag(choice)
                            }
                        }

                        Picker("Response style", selection: $piResponseStyle) {
                            Text("Daemon Default").tag(nil as PiResponseStyle?)
                            ForEach(PiResponseStyle.allCases) { style in
                                Text(style.title).tag(style as PiResponseStyle?)
                            }
                        }
                    }
                }

                if agent.family == .opencode {
                    Section("OpenCode") {
                        TextField("Provider", text: $openCodeProvider)

                        TextField("Execution mode", text: $openCodeExecutionMode)
                    }
                }

                Section {
                    Text(descriptionText)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .formStyle(.grouped)
        }
        .frame(width: 480, height: 600)
        .toolbar {
            ToolbarItemGroup(placement: .cancellationAction) {
                Button("Cancel") {
                    dismiss()
                }
                .keyboardShortcut(.cancelAction)
            }
            ToolbarItemGroup(placement: .confirmationAction) {
                Button("Save") {
                    saveChanges()
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .sheet(isPresented: $showImagePicker) {
            ImagePickerView(image: $selectedImage)
        }
        .alert("Remove Photo?", isPresented: $showRemovePhotoAlert) {
            Button("Remove", role: .destructive) {
                selectedImage = nil
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This will remove the custom avatar and show the default icon.")
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Agent Settings")
                .font(.headline)
            Text(agent.displayName)
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding()
    }

    private var avatarSection: some View {
        VStack(spacing: 16) {
            ZStack(alignment: .bottomTrailing) {
                avatarPreview
                    .frame(width: 100, height: 100)

                Button {
                    if selectedImage != nil {
                        showRemovePhotoAlert = true
                    } else {
                        showImagePicker = true
                    }
                } label: {
                    ZStack {
                        Circle()
                            .fill(Color.accentColor)
                            .frame(width: 32, height: 32)

                        Image(systemName: selectedImage != nil ? "pencil" : "plus")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(.white)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
    }

    private var avatarPreview: some View {
        Group {
            if let image = selectedImage {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .clipShape(Circle())
            } else if let assetName = agent.defaultAvatarAssetName {
                AgentDefaultAvatarArtwork(
                    assetName: assetName,
                    size: 100,
                    shape: .circle
                )
            } else {
                Circle()
                    .fill(AgentAvatarPalette.tintColor(named: agent.tintName).opacity(0.14))
                    .overlay {
                        Image(systemName: agent.symbolName)
                            .font(.system(size: 36, weight: .semibold))
                            .foregroundStyle(AgentAvatarPalette.tintColor(named: agent.tintName))
                    }
            }
        }
    }

    private func saveChanges() {
        let normalizedName = trimmedValue(displayName)

        let processedData: Data? = selectedImage.flatMap { processImage($0) }

        let settings = AgentLocalSettings(
            model: trimmedValue(model),
            systemPrompt: trimmedValue(systemPrompt),
            codexReasoningEffort: agent.family == .codex ? codexReasoningEffort : nil,
            codexApprovalPolicy: agent.family == .codex ? codexApprovalPolicy : nil,
            claudeThinkingMode: agent.family == .claude ? claudeThinkingMode : nil,
            claudeFallbackModel: agent.family == .claude ? trimmedValue(claudeFallbackModel) : nil,
            piProfile: agent.family == .pi ? trimmedValue(piProfile) : nil,
            piQuietStartup: agent.family == .pi ? piQuietStartupChoice.value : nil,
            piResponseStyle: agent.family == .pi ? piResponseStyle : nil,
            openCodeProvider: agent.family == .opencode ? trimmedValue(openCodeProvider) : nil,
            openCodeExecutionMode: agent.family == .opencode ? trimmedValue(openCodeExecutionMode) : nil
        ).normalized

        onSave(normalizedName, processedData, settings.isEmpty ? nil : settings)
        dismiss()
    }

    private var modelPlaceholder: String {
        switch agent.family {
        case .codex:
            return "Preferred model, for example gpt-5.4"
        case .claude:
            return "Preferred model, for example claude-sonnet"
        case .pi:
            return "Preferred model, for example pi-4"
        case .opencode:
            return "Preferred model, for example gpt-4.1"
        case .human, .generic:
            return "Preferred model"
        }
    }

    private var descriptionText: String {
        switch agent.family {
        case .codex:
            return "Name and avatar change the local thread UI. Runtime fields store your Codex presets, including model, reasoning level, and approval behavior."
        case .claude:
            return "Name and avatar change the local thread UI. Claude Code keeps its own model, thinking mode, and fallback model preset here."
        case .pi:
            return "Name and avatar change the local thread UI. Pi keeps model, profile, quiet-startup, and response-style preferences here."
        case .opencode:
            return "Name and avatar change the local thread UI. OpenCode keeps provider and execution-mode presets here."
        case .human, .generic:
            return "These settings are stored locally so this participant keeps a stable profile across threads."
        }
    }

    private func trimmedValue(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private func processImage(_ image: NSImage) -> Data? {
        guard let tiffData = image.tiffRepresentation,
              let bitmap = NSBitmapImageRep(data: tiffData) else {
            return nil
        }
        return bitmap.representation(using: .jpeg, properties: [.compressionFactor: 0.8])
    }
}

struct ImagePickerView: View {
    @Binding var image: NSImage?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 16) {
            Text("Select an Image")
                .font(.headline)

            Button("Choose Image...") {
                let panel = NSOpenPanel()
                panel.allowedContentTypes = [.image]
                panel.allowsMultipleSelection = false
                panel.canChooseDirectories = false

                if panel.runModal() == .OK, let url = panel.url {
                    if let nsImage = NSImage(contentsOf: url) {
                        image = nsImage
                    }
                }
                dismiss()
            }
            .buttonStyle(.borderedProminent)

            Button("Cancel", role: .cancel) {
                dismiss()
            }
        }
        .padding(24)
        .frame(width: 280, height: 120)
    }
}
