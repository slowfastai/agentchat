import SwiftUI

struct CreateArtifactSheet: View {
    @EnvironmentObject private var store: DemoStore
    @Environment(\.dismiss) private var dismiss

    let issueID: UUID
    let thread: Thread?

    @State private var kind: IssueArtifactKind = .note
    @State private var title = ""
    @State private var summary = ""
    @State private var pathOrURL = ""

    var body: some View {
        VStack(spacing: 0) {
            header(title: "Add Artifact")

            Divider()

            Form {
                Section("Kind") {
                    Picker("Kind", selection: $kind) {
                        ForEach(IssueArtifactKind.allCases) { kind in
                            Text(kind.title).tag(kind)
                        }
                    }
                }

                Section("Details") {
                    TextField("Title", text: $title)
                    TextField("Path or URL", text: $pathOrURL)
                    TextEditor(text: $summary)
                        .frame(height: 90)
                }

                if let thread {
                    Section("Source Thread") {
                        Text(thread.title)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .formStyle(.grouped)

            Divider()

            footer(buttonTitle: "Save Artifact", isDisabled: title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty) {
                store.addArtifact(
                    issueID: issueID,
                    threadID: thread?.id,
                    kind: kind,
                    title: title,
                    summary: summary,
                    pathOrURL: pathOrURL
                )
                dismiss()
            }
        }
        .frame(width: 480, height: 420)
        .onAppear {
            if let thread, title.isEmpty {
                title = "\(thread.title) artifact"
                summary = thread.latestActivityText
            }
        }
    }

    @ViewBuilder
    private func header(title: String) -> some View {
        HStack {
            Text(title)
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

    @ViewBuilder
    private func footer(buttonTitle: String, isDisabled: Bool, action: @escaping () -> Void) -> some View {
        HStack {
            Spacer()
            Button("Cancel") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)

            Button(buttonTitle, action: action)
                .keyboardShortcut(.defaultAction)
                .disabled(isDisabled)
        }
        .padding()
    }
}

struct CreateDecisionSheet: View {
    @EnvironmentObject private var store: DemoStore
    @Environment(\.dismiss) private var dismiss

    let issueID: UUID
    let thread: Thread?

    @State private var title = ""
    @State private var rationale = ""

    var body: some View {
        VStack(spacing: 0) {
            header(title: "Add Decision")

            Divider()

            Form {
                Section("Decision") {
                    TextField("Title", text: $title)
                    TextEditor(text: $rationale)
                        .frame(height: 120)
                }

                if let thread {
                    Section("Source Thread") {
                        Text(thread.title)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .formStyle(.grouped)

            Divider()

            footer(buttonTitle: "Save Decision", isDisabled: isSaveDisabled) {
                store.addDecision(
                    issueID: issueID,
                    threadID: thread?.id,
                    title: title,
                    rationale: rationale
                )
                dismiss()
            }
        }
        .frame(width: 480, height: 360)
        .onAppear {
            if let thread, title.isEmpty {
                title = "Decision from \(thread.title)"
                rationale = thread.latestActivityText
            }
        }
    }

    private var isSaveDisabled: Bool {
        title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ||
        rationale.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    @ViewBuilder
    private func header(title: String) -> some View {
        HStack {
            Text(title)
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

    @ViewBuilder
    private func footer(buttonTitle: String, isDisabled: Bool, action: @escaping () -> Void) -> some View {
        HStack {
            Spacer()
            Button("Cancel") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)

            Button(buttonTitle, action: action)
                .keyboardShortcut(.defaultAction)
                .disabled(isDisabled)
        }
        .padding()
    }
}
