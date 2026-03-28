import SwiftUI
#if os(iOS)
import UIKit
#endif

struct AgentEditSheet: View {
    let agent: DaemonAgentSummary
    let onSave: (DaemonAgentSummary) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var displayName: String
    @State private var selectedImage: UIImage?
    @State private var showImagePicker = false
    @State private var showRemovePhotoAlert = false

    init(agent: DaemonAgentSummary, onSave: @escaping (DaemonAgentSummary) -> Void) {
        self.agent = agent
        self.onSave = onSave
        _displayName = State(initialValue: agent.customDisplayName ?? "")
        if let imageData = agent.avatarImageData {
            _selectedImage = State(initialValue: UIImage(data: imageData))
        }
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    avatarSection
                }

                Section("Display Name") {
                    TextField("Enter a custom name", text: $displayName)
                        .textInputAutocapitalization(.words)
                }

                Section {
                    Text("Customize how this agent appears in your list. The original agent name from the daemon will still be used for identification.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
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
        updatedAgent = updatedAgent.withCustomDisplayName(displayName.isEmpty ? nil : displayName)

        if let image = selectedImage {
            let processedData = processImage(image)
            updatedAgent = updatedAgent.withAvatarImageData(processedData)
        } else {
            updatedAgent = updatedAgent.withAvatarImageData(nil)
        }

        onSave(updatedAgent)
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
