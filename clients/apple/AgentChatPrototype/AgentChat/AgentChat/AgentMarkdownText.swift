import SwiftUI

struct AgentMarkdownText: View {
    let content: String
    var preferredSyntax: AttributedString.MarkdownParsingOptions.InterpretedSyntax = .full

    var body: some View {
        Group {
            if let attributed = parsedContent {
                Text(attributed)
            } else {
                Text(content)
            }
        }
        .textSelection(.enabled)
    }

    private var parsedContent: AttributedString? {
        guard #available(iOS 15.0, *) else {
            return nil
        }

        let preferredOptions = AttributedString.MarkdownParsingOptions(
            interpretedSyntax: preferredSyntax
        )

        if let preferred = try? AttributedString(markdown: content, options: preferredOptions) {
            return preferred
        }

        let inlineOptions = AttributedString.MarkdownParsingOptions(
            interpretedSyntax: .inlineOnlyPreservingWhitespace
        )

        return try? AttributedString(markdown: content, options: inlineOptions)
    }
}
