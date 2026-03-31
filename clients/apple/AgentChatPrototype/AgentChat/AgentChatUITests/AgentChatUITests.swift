//
//  AgentChatUITests.swift
//  AgentChatUITests
//
//  Created by Jia Li on 2026/3/24.
//

import XCTest

final class AgentChatUITests: XCTestCase {
    private enum AvatarSettingsHostMode: String {
        case root
        case local
    }

    private func elapsedMS(for label: String, in traceLog: String) -> String? {
        let pattern = #"\[UITrace\] END "# + NSRegularExpression.escapedPattern(for: label) + #" elapsed_ms=([0-9]+(?:\.[0-9]+)?)"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else {
            return nil
        }

        let nsRange = NSRange(traceLog.startIndex..., in: traceLog)
        guard let match = regex.firstMatch(in: traceLog, options: [], range: nsRange),
              let valueRange = Range(match.range(at: 1), in: traceLog) else {
            return nil
        }

        return String(traceLog[valueRange])
    }

    private func traceSummary(from traceLog: String) -> String {
        [
            "thread_tap_to_detail_visible",
            "thread_tap_to_composer_ready",
            "composer_tap_to_focus",
            "composer_tap_to_keyboard",
            "composer_focus_to_keyboard"
        ]
        .map { label in
            let elapsed = elapsedMS(for: label, in: traceLog) ?? "missing"
            return "\(label)=\(elapsed)ms"
        }
        .joined(separator: "\n")
    }

    override func setUpWithError() throws {
        // Put setup code here. This method is called before the invocation of each test method in the class.

        // In UI tests it is usually best to stop immediately when a failure occurs.
        continueAfterFailure = false

        // In UI tests it’s important to set the initial state - such as interface orientation - required for your tests before they run. The setUp method is a good place to do this.
    }

    override func tearDownWithError() throws {
        // Put teardown code here. This method is called after the invocation of each test method in the class.
    }

    @MainActor
    func testExample() throws {
        let app = XCUIApplication()
        app.launch()

        XCTAssertTrue(app.tabBars.buttons["Feed"].waitForExistence(timeout: 3))
        XCTAssertTrue(app.tabBars.buttons["Agents"].exists)
        XCTAssertTrue(app.tabBars.buttons["Settings"].exists)
        XCTAssertTrue(app.tabBars.buttons["Search"].exists)
        XCTAssertTrue(app.navigationBars["Feed"].waitForExistence(timeout: 2))
    }

    @MainActor
    func testLaunchPerformance() throws {
        // This measures how long it takes to launch your application.
        measure(metrics: [XCTApplicationLaunchMetric()]) {
            XCUIApplication().launch()
        }
    }

    @MainActor
    func testAvatarSettingsSheetLatencyProbe() throws {
        let app = XCUIApplication()
        app.launchArguments += ["UITestSeedAvatarLatency"]

        let hostMode = ProcessInfo.processInfo.environment["AGENTCHAT_AVATAR_SETTINGS_HOST_MODE"]
            .flatMap(AvatarSettingsHostMode.init(rawValue:))
            ?? .local
        if hostMode == .root {
            app.launchArguments += ["UITestAvatarSettingsHostRoot"]
        }

        app.launch()

        let threadButton = app.buttons.containing(.staticText, identifier: "Avatar Latency Thread").firstMatch
        XCTAssertTrue(threadButton.waitForExistence(timeout: 5), "Seeded thread row should exist")
        threadButton.tap()

        let avatarButton = app.buttons["Open Codex settings"]
        XCTAssertTrue(avatarButton.waitForExistence(timeout: 5), "Codex avatar button should exist")
        avatarButton.tap()

        let settingsTitle = app.navigationBars["Agent Settings"]
        XCTAssertTrue(settingsTitle.waitForExistence(timeout: 5), "Agent settings sheet should appear")
    }

    @MainActor
    func testThreadComposerLatencyProbe() throws {
        let app = XCUIApplication()
        app.launchArguments += ["UITestSeedThreadComposerLatency", "UITestExposePerformanceProbe"]
        app.launch()

        let probeLog = app.staticTexts["UIPerformanceProbeLog"]
        XCTAssertTrue(probeLog.waitForExistence(timeout: 5), "Performance probe log should exist")

        let threadButton = app.buttons.containing(.staticText, identifier: "Composer Latency Thread").firstMatch
        XCTAssertTrue(threadButton.waitForExistence(timeout: 5), "Seeded thread row should exist")
        threadButton.tap()

        let composerTextView = app.textViews["Message"].firstMatch
        let composerTextField = app.textFields["Message"].firstMatch
        let composer = composerTextView.waitForExistence(timeout: 3) ? composerTextView : composerTextField
        XCTAssertTrue(composer.waitForExistence(timeout: 5), "Composer text input should appear")
        composer.tap()

        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5), "Keyboard should appear")

        let hasKeyboardTrace = NSPredicate(format: "label CONTAINS %@", "composer_tap_to_keyboard")
        expectation(for: hasKeyboardTrace, evaluatedWith: probeLog)
        waitForExpectations(timeout: 5)

        let traceLog = probeLog.label
        let summary = traceSummary(from: traceLog)
        let attachment = XCTAttachment(string: traceLog + "\n\n" + summary)
        attachment.name = "ThreadComposerLatencyTrace"
        attachment.lifetime = .keepAlways
        add(attachment)
        print(summary)

        XCTAssertTrue(traceLog.contains("thread_tap_to_detail_visible"))
        XCTAssertTrue(traceLog.contains("thread_tap_to_composer_ready"))
        XCTAssertTrue(traceLog.contains("composer_tap_to_focus"))
        XCTAssertTrue(traceLog.contains("composer_tap_to_keyboard"))
        XCTAssertTrue(traceLog.contains("composer_focus_to_keyboard"))
    }
}
