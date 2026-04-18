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

## Run the prototype

The repo now includes a runnable macOS prototype project under [`Runner/`](./Runner).

Generate the Xcode project from the checked-in spec:

```bash
cd clients/apple/AgentChatPrototype/Runner
xcodegen generate
```

Build from the command line:

```bash
xcodebuild \
  -project AgentChatPrototypeRunner.xcodeproj \
  -scheme AgentChatPrototypeRunner \
  -configuration Debug \
  build
```

Or open [`AgentChatPrototypeRunner.xcodeproj`](./Runner/AgentChatPrototypeRunner.xcodeproj) in Xcode after generation and run the `AgentChatPrototypeRunner` scheme.

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
