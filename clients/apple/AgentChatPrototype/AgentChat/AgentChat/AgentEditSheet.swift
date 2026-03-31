import SwiftUI
#if os(iOS)
import UIKit
#endif

struct AgentEditSheet: View {
    let agent: DaemonAgentSummary
    let initialSettings: AgentLocalSettings
    let onSave: (DaemonAgentSummary, AgentLocalSettings?) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var displayName: String
    @State private var selectedImage: UIImage?
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
        onSave: @escaping (DaemonAgentSummary, AgentLocalSettings?) -> Void
    ) {
        self.agent = agent
        self.initialSettings = initialSettings.normalized
        self.onSave = onSave
        _displayName = State(initialValue: agent.customDisplayName ?? "")
        if let imageData = agent.avatarImageData {
            _selectedImage = State(initialValue: UIImage(data: imageData))
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
        NavigationStack {
            Form {
                Section {
                    avatarSection
                }

                Section("Identity") {
                    TextField("Enter a custom name", text: $displayName)
                        .textInputAutocapitalization(.words)
                }

                Section("Runtime") {
                    TextField(modelPlaceholder, text: $model)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()

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
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                    }
                }

                if agent.family == .pi {
                    Section("Pi") {
                        TextField("Profile or config preset", text: $piProfile)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()

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
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()

                        TextField("Execution mode", text: $openCodeExecutionMode)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                    }
                }

                Section {
                    Text(descriptionText)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Agent Settings")
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
                }
            }
            .sheet(isPresented: $showImagePicker) {
                ImagePicker(image: $selectedImage)
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
                Image(uiImage: image)
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
        var updatedAgent = agent
        let normalizedName = trimmedValue(displayName)
        updatedAgent = updatedAgent.withCustomDisplayName(normalizedName)

        if let image = selectedImage {
            let processedData = processImage(image)
            updatedAgent = updatedAgent.withAvatarImageData(processedData)
        } else {
            updatedAgent = updatedAgent.withAvatarImageData(nil)
        }

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

        onSave(updatedAgent, settings.isEmpty ? nil : settings)
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

    private func processImage(_ image: UIImage) -> Data? {
        let targetSize = CGSize(width: 200, height: 200)
        let squareImage = cropToSquare(image)

        let renderer = UIGraphicsImageRenderer(size: targetSize)
        let resizedImage = renderer.image { _ in
            squareImage.draw(in: CGRect(origin: .zero, size: targetSize))
        }

        return resizedImage.jpegData(compressionQuality: 0.8)
    }

    private func cropToSquare(_ image: UIImage) -> UIImage {
        let originalSize = image.size
        let sideLength = min(originalSize.width, originalSize.height)

        let x = (originalSize.width - sideLength) / 2
        let y = (originalSize.height - sideLength) / 2

        let cropRect = CGRect(x: x, y: y, width: sideLength, height: sideLength)

        guard let cgImage = image.cgImage?.cropping(to: cropRect) else {
            return image
        }

        return UIImage(cgImage: cgImage, scale: image.scale, orientation: image.imageOrientation)
    }
}

struct ImagePicker: UIViewControllerRepresentable {
    @Binding var image: UIImage?
    @Environment(\.dismiss) private var dismiss

    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.delegate = context.coordinator
        picker.allowsEditing = false
        return picker
    }

    func updateUIViewController(_ uiViewController: UIImagePickerController, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        let parent: ImagePicker

        init(_ parent: ImagePicker) {
            self.parent = parent
        }

        func imagePickerController(_ picker: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
            if let image = info[.originalImage] as? UIImage {
                parent.image = image
            }
            parent.dismiss()
        }

        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
            parent.dismiss()
        }
    }
}
