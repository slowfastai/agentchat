#!/bin/zsh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
PROJECT_PATH="$SCRIPT_DIR/AgentChat/AgentChat.xcodeproj"
DERIVED_DATA_PATH="${AGENTCHAT_DERIVED_DATA_PATH:-/private/tmp/agentchat-desktop-release-derived}"
OUTPUT_DIR="${AGENTCHAT_OUTPUT_DIR:-$REPO_ROOT/build/macos}"
APP_PATH="$DERIVED_DATA_PATH/Build/Products/Release/AgentChatDesktop.app"
DAEMON_PATH="$REPO_ROOT/daemon/target/release/agentchat-daemon"

echo "Building agentchat-daemon release binary..."
cargo build --release --manifest-path "$REPO_ROOT/daemon/Cargo.toml" -p agentchat-daemon

echo "Building AgentChatDesktop.app..."
xcodebuild \
  -project "$PROJECT_PATH" \
  -scheme AgentChatDesktop \
  -configuration Release \
  -sdk macosx \
  CODE_SIGNING_ALLOWED=NO \
  ENABLE_PREVIEWS=NO \
  SWIFT_ENABLE_PREVIEWS=NO \
  -derivedDataPath "$DERIVED_DATA_PATH" \
  build

RESOURCE_DIR="$APP_PATH/Contents/SharedSupport"
mkdir -p "$RESOURCE_DIR" "$OUTPUT_DIR"
cp "$DAEMON_PATH" "$RESOURCE_DIR/agentchat-daemon"
chmod 755 "$RESOURCE_DIR/agentchat-daemon"

OUTPUT_APP_PATH="$OUTPUT_DIR/AgentChatDesktop.app"
ditto "$APP_PATH" "$OUTPUT_APP_PATH"

echo "Built: $OUTPUT_APP_PATH"
echo "The app is unsigned; sign and notarize it before distribution."
