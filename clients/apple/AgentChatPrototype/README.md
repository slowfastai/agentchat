# AgentChatPrototype

SwiftUI code skeleton for a macOS + iPad-first prototype.

## What is included

- `App/` root app entry and split-view navigation
- `Assets/AgentAvatars/` original source avatar artwork; app-ready copies live in `AgentChat/AgentChat/Assets.xcassets/`
- `Core/Models/` prototype data models
- `Core/Store/` mock in-memory demo store with fake streaming responses
- `DesignSystem/` shared colors, cards, pills, badges, avatars
- `Features/Inbox/` issue inbox list
- `Features/Workspace/` issue workspace with chat, side panel, and composer
- `Features/Switcher/` multi-task switcher
- `Features/Agents/` agent list

## Suggested Xcode setup

Create a new **App** project in Xcode:

- Product Name: `AgentChatPrototype`
- Interface: `SwiftUI`
- Language: `Swift`
- Platforms: `iOS` + `macOS` (or start with macOS first)
- Deployment target: iOS 17 / macOS 14 or newer

Then add all files under this folder into the app target.

## Demo flow

The mock store already seeds three demo issues:

- `#128 Review relay reconnect recovery`
- `#135 Fix ws shutdown cleanup race`
- `#141 Distill session knowledge into skills`

In the workspace screen, type a message and hit **Send**. The demo store will simulate:

- user message
- thinking event
- tool call
- streaming agent response
- turn end

## Suggested next step after this skeleton

1. Tweak layout and spacing until the product feels exciting.
2. Replace the mock `DemoStore.sendMessage(...)` path with real WebSocket messages.
3. Add `list_agents` and `create_session(agent_id)` to the daemon protocol.
