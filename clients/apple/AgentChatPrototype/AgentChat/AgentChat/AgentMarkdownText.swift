import SwiftUI
#if os(iOS)
import UIKit
#endif

#if os(iOS)
private enum AgentMarkdownRenderCache {
    private static let cache = NSCache<NSString, NSAttributedString>()

    static func renderedText(
        for content: String,
        preferredSyntax: AttributedString.MarkdownParsingOptions.InterpretedSyntax,
        textStyle: UIFont.TextStyle,
        textColor: UIColor,
        lineSpacing: CGFloat
    ) -> NSAttributedString {
        let cacheKey = [
            String(describing: preferredSyntax),
            textStyle.rawValue,
            String(describing: textColor),
            String(format: "%.2f", lineSpacing),
            content
        ].joined(separator: "|") as NSString

        if let cachedText = cache.object(forKey: cacheKey) {
            return cachedText
        }

        let renderedText = AgentMarkdownText.makeRenderedAttributedText(
            content: content,
            preferredSyntax: preferredSyntax,
            textStyle: textStyle,
            textColor: textColor,
            lineSpacing: lineSpacing
        )
        cache.setObject(renderedText, forKey: cacheKey)
        return renderedText
    }
}
#endif

struct AgentMarkdownText: View {
    let content: String
    var preferredSyntax: AttributedString.MarkdownParsingOptions.InterpretedSyntax = .full
    #if os(iOS)
    var textStyle: UIFont.TextStyle = .body
    var textColor: UIColor = .label
    var lineSpacing: CGFloat = 0
    #endif

    var body: some View {
        #if os(iOS)
        AgentSelectableText(
            attributedText: renderedAttributedText,
            lineSpacing: lineSpacing
        )
        #else
        Group {
            if let attributed = parsedContent {
                Text(attributed)
            } else {
                Text(content)
            }
        }
        .textSelection(.enabled)
        #endif
    }

    private var parsedContent: AttributedString? {
        Self.makeParsedContent(content: content, preferredSyntax: preferredSyntax)
    }

    fileprivate static func makeParsedContent(
        content: String,
        preferredSyntax: AttributedString.MarkdownParsingOptions.InterpretedSyntax
    ) -> AttributedString? {
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

    #if os(iOS)
    private var renderedAttributedText: NSAttributedString {
        AgentMarkdownRenderCache.renderedText(
            for: content,
            preferredSyntax: preferredSyntax,
            textStyle: textStyle,
            textColor: textColor,
            lineSpacing: lineSpacing
        )
    }

    fileprivate static func makeRenderedAttributedText(
        content: String,
        preferredSyntax: AttributedString.MarkdownParsingOptions.InterpretedSyntax,
        textStyle: UIFont.TextStyle,
        textColor: UIColor,
        lineSpacing: CGFloat
    ) -> NSAttributedString {
        if let parsedContent = makeParsedContent(content: content, preferredSyntax: preferredSyntax) {
            return AgentSelectableText.styledMarkdown(
                parsedContent,
                fallbackTextStyle: textStyle,
                textColor: textColor,
                lineSpacing: lineSpacing
            )
        }

        return AgentSelectableText.styledPlainText(
            content,
            font: AgentSelectableText.preferredFont(for: textStyle),
            textColor: textColor,
            lineSpacing: lineSpacing
        )
    }
    #endif
}

#if os(iOS)
struct AgentSelectableText: View {
    private let attributedText: NSAttributedString

    init(
        _ content: String,
        font: UIFont = AgentSelectableText.preferredFont(for: .body),
        textColor: UIColor = .label,
        lineSpacing: CGFloat = 0
    ) {
        self.attributedText = Self.styledPlainText(
            content,
            font: font,
            textColor: textColor,
            lineSpacing: lineSpacing
        )
    }

    init(attributedText: NSAttributedString, lineSpacing: CGFloat = 0) {
        self.attributedText = Self.applyingLineSpacing(lineSpacing, to: attributedText)
    }

    var body: some View {
        NativeSelectableTextView(attributedText: attributedText)
    }

    static func preferredFont(for textStyle: UIFont.TextStyle) -> UIFont {
        UIFont.preferredFont(forTextStyle: textStyle)
    }

    static func preferredMonospacedFont(for textStyle: UIFont.TextStyle) -> UIFont {
        let pointSize = UIFont.preferredFont(forTextStyle: textStyle).pointSize
        let base = UIFont.monospacedSystemFont(ofSize: pointSize, weight: .regular)
        return UIFontMetrics(forTextStyle: textStyle).scaledFont(for: base)
    }

    static func styledPlainText(
        _ content: String,
        font: UIFont,
        textColor: UIColor,
        lineSpacing: CGFloat
    ) -> NSAttributedString {
        let paragraphStyle = NSMutableParagraphStyle()
        paragraphStyle.lineSpacing = lineSpacing

        return NSAttributedString(
            string: content,
            attributes: [
                .font: font,
                .foregroundColor: textColor,
                .paragraphStyle: paragraphStyle
            ]
        )
    }

    static func styledMarkdown(
        _ content: AttributedString,
        fallbackTextStyle: UIFont.TextStyle,
        textColor: UIColor,
        lineSpacing: CGFloat
    ) -> NSAttributedString {
        let mutable = NSMutableAttributedString(
            attributedString: NSAttributedString(content)
        )
        let fullRange = NSRange(location: 0, length: mutable.length)

        mutable.addAttribute(.foregroundColor, value: textColor, range: fullRange)

        mutable.enumerateAttribute(.font, in: fullRange) { value, range, _ in
            guard value == nil else { return }
            mutable.addAttribute(
                .font,
                value: preferredFont(for: fallbackTextStyle),
                range: range
            )
        }

        return applyingLineSpacing(lineSpacing, to: mutable)
    }

    static func applyingLineSpacing(
        _ lineSpacing: CGFloat,
        to attributedText: NSAttributedString
    ) -> NSAttributedString {
        guard lineSpacing > 0 else { return attributedText }

        let mutable = NSMutableAttributedString(attributedString: attributedText)
        let fullRange = NSRange(location: 0, length: mutable.length)

        mutable.enumerateAttribute(.paragraphStyle, in: fullRange) { value, range, _ in
            let paragraphStyle =
                ((value as? NSParagraphStyle)?.mutableCopy() as? NSMutableParagraphStyle)
                ?? NSMutableParagraphStyle()
            paragraphStyle.lineSpacing = lineSpacing
            mutable.addAttribute(.paragraphStyle, value: paragraphStyle, range: range)
        }

        return mutable
    }
}

private struct NativeSelectableTextView: UIViewRepresentable {
    let attributedText: NSAttributedString

    func makeUIView(context: Context) -> UITextView {
        let textView = UITextView()
        textView.backgroundColor = .clear
        textView.isEditable = false
        textView.isSelectable = true
        textView.isScrollEnabled = false
        textView.adjustsFontForContentSizeCategory = true
        textView.textContainerInset = .zero
        textView.textContainer.lineFragmentPadding = 0
        textView.textContainer.widthTracksTextView = true
        textView.showsVerticalScrollIndicator = false
        textView.showsHorizontalScrollIndicator = false
        textView.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        textView.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return textView
    }

    func updateUIView(_ uiView: UITextView, context: Context) {
        if !(uiView.attributedText?.isEqual(to: attributedText) ?? false) {
            uiView.attributedText = attributedText
        }
    }

    func sizeThatFits(
        _ proposal: ProposedViewSize,
        uiView: UITextView,
        context: Context
    ) -> CGSize? {
        guard let width = proposal.width else { return nil }
        let fittingSize = uiView.sizeThatFits(
            CGSize(width: width, height: .greatestFiniteMagnitude)
        )
        return CGSize(width: width, height: ceil(fittingSize.height))
    }
}
#endif
