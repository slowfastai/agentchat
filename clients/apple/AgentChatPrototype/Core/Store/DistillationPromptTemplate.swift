import Foundation

private struct DistillationPromptTemplateConfig: Decodable {
    var templateVersion: String
    var transcriptLineLimit: Int
    var systemInstructions: [String]
    var jsonShape: String
    var template: String

    enum CodingKeys: String, CodingKey {
        case templateVersion = "template_version"
        case transcriptLineLimit = "transcript_line_limit"
        case systemInstructions = "system_instructions"
        case jsonShape = "json_shape"
        case template
    }
}

enum DistillationPromptTemplate {
    static func currentTemplateVersion(
        family: DistillationTemplateFamily = .default,
        agentIdentifier: String? = nil
    ) -> String {
        load(family: family, agentIdentifier: agentIdentifier).templateVersion
    }

    private static func load(
        family: DistillationTemplateFamily = .default,
        agentIdentifier: String? = nil
    ) -> DistillationPromptTemplateConfig {
        if family != .default,
           let familySpecific = loadNamedTemplate("distillation_prompt_template_\(family.rawValue)") {
            return familySpecific
        }

        if let agentIdentifier,
           let agentSpecific = loadNamedTemplate("distillation_prompt_template_\(normalizedAgentIdentifier(agentIdentifier))") {
            return agentSpecific
        }

        if let common = loadNamedTemplate("distillation_prompt_template") {
            return common
        }
        return fallback
    }

    private static func loadNamedTemplate(_ resourceName: String) -> DistillationPromptTemplateConfig? {
        if let url = Bundle.main.url(forResource: resourceName, withExtension: "json"),
           let data = try? Data(contentsOf: url),
           let config = try? JSONDecoder().decode(DistillationPromptTemplateConfig.self, from: data) {
            return config
        }
        return nil
    }

    static func render(
        issueNumber: Int,
        issueTitle: String,
        issueSummary: String,
        threadTitle: String,
        threadPurpose: String,
        transcriptLines: [String],
        family: DistillationTemplateFamily = .default,
        agentIdentifier: String? = nil
    ) -> String {
        let config = load(family: family, agentIdentifier: agentIdentifier)
        let limitedTranscript = transcriptLines
            .suffix(max(1, config.transcriptLineLimit))
            .joined(separator: "\n")

        let body = config.template
            .replacingOccurrences(of: "{{issue_number}}", with: String(issueNumber))
            .replacingOccurrences(of: "{{issue_title}}", with: issueTitle)
            .replacingOccurrences(of: "{{issue_summary}}", with: issueSummary)
            .replacingOccurrences(of: "{{thread_title}}", with: threadTitle)
            .replacingOccurrences(of: "{{thread_purpose}}", with: threadPurpose)
            .replacingOccurrences(of: "{{transcript}}", with: limitedTranscript)

        return ([config.systemInstructions.joined(separator: "\n"), "", "JSON shape:", config.jsonShape, "", body])
            .joined(separator: "\n")
    }

    private static func normalizedAgentIdentifier(_ identifier: String) -> String {
        identifier
            .lowercased()
            .replacingOccurrences(of: " ", with: "_")
            .replacingOccurrences(of: "-", with: "_")
    }

    private static let fallback = DistillationPromptTemplateConfig(
        templateVersion: "distillation.v1",
        transcriptLineLimit: 24,
        systemInstructions: [
            "You are helping distill an engineering thread into project-management outputs.",
            "Return strict JSON only.",
            "Keep each field concise and useful.",
            "If a field is not justified, return an empty string for its text fields."
        ],
        jsonShape: """
        {
          "summary": "string",
          "decision": { "title": "string", "rationale": "string" },
          "artifact": { "kind": "note|document|changedFile|testLog|branch|commit|pullRequest|screenshot", "title": "string", "summary": "string", "pathOrURL": "string or empty" },
          "followUp": { "title": "string", "summary": "string", "priority": "low|medium|high|urgent" }
        }
        """,
        template: """
        Issue:
        #{{issue_number}} {{issue_title}}
        Summary: {{issue_summary}}

        Thread:
        {{thread_title}}
        Purpose: {{thread_purpose}}

        Transcript:
        {{transcript}}
        """
    )
}
