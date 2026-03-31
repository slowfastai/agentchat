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
}
