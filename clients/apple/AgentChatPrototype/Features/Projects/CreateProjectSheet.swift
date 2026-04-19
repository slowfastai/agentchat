#if os(macOS)
import SwiftUI
import AppKit

struct CreateProjectSheet: View {
    @EnvironmentObject private var store: DemoStore
    @Environment(\.dismiss) private var dismiss

    @State private var projectName = ""
    @State private var repoPath = ""
    @State private var selectedColor: ColorToken = .blue
    @State private var distillationTemplateFamily: DistillationTemplateFamily = .default
    @State private var isValid = false

    var body: some View {
        VStack(spacing: 0) {
            header
            
            Divider()
            
            formContent
            
            Divider()
            
            footer
        }
        .frame(width: 480, height: 420)
        .onChange(of: projectName) { _, _ in validateForm() }
        .onChange(of: repoPath) { _, _ in validateForm() }
    }

    private var header: some View {
        HStack {
            Text("New Project")
                .font(.headline)
            Spacer()
            Button {
                dismiss()
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
        }
        .padding()
    }

    private var formContent: some View {
        Form {
            Section {
                TextField("Project Name", text: $projectName)
                    .textFieldStyle(.roundedBorder)
            } header: {
                Text("Name")
            }

            Section {
                HStack {
                    TextField("Repository Path", text: $repoPath)
                        .textFieldStyle(.roundedBorder)
                        .disabled(true)

                    Button("Choose...") {
                        selectDirectory()
                    }
                }
                Text("Select the git repository folder for this project")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } header: {
                Text("Repository")
            }

            Section {
                Picker("Color", selection: $selectedColor) {
                    ForEach(ColorToken.allCases, id: \.self) { color in
                        HStack {
                            Circle()
                                .fill(Color(color))
                                .frame(width: 12, height: 12)
                            Text(color.rawValue.capitalized)
                        }
                        .tag(color)
                    }
                }
                .pickerStyle(.segmented)
            } header: {
                Text("Color")
            }

            Section {
                Picker("Template", selection: $distillationTemplateFamily) {
                    ForEach(DistillationTemplateFamily.allCases) { family in
                        Text(family.title).tag(family)
                    }
                }
                .pickerStyle(.segmented)
            } header: {
                Text("Distillation")
            }
        }
        .formStyle(.grouped)
    }

    private var footer: some View {
        HStack {
            Spacer()
            Button("Cancel") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)

            Button("Create") {
                createProject()
            }
            .keyboardShortcut(.defaultAction)
            .disabled(!isValid)
        }
        .padding()
    }

    private func selectDirectory() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = true
        panel.directoryURL = FileManager.default.homeDirectoryForCurrentUser
        panel.prompt = "Select"
        panel.message = "Choose a folder to use as the project repository"

        if panel.runModal() == .OK, let url = panel.url {
            repoPath = url.path
            if projectName.isEmpty {
                projectName = url.lastPathComponent
            }
        }
    }

    private func validateForm() {
        isValid = !projectName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
                  !repoPath.isEmpty &&
                  FileManager.default.fileExists(atPath: repoPath)
    }

    private func createProject() {
        store.createProject(
            name: projectName.trimmingCharacters(in: .whitespacesAndNewlines),
            repoPath: repoPath,
            color: selectedColor,
            distillationTemplateFamily: distillationTemplateFamily
        )
        dismiss()
    }
}
#endif
