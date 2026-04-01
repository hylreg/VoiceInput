#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

if ! command -v cargo >/dev/null 2>&1; then
  if command -v rustup >/dev/null 2>&1; then
    cargo_path="$(rustup which cargo 2>/dev/null || true)"
    if [[ -n "${cargo_path:-}" ]]; then
      export PATH="$(cd "$(dirname "$cargo_path")" && pwd):$PATH"
    fi
  fi
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "未找到 cargo。请先安装 Rust 工具链。" >&2
  exit 1
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "这个脚本只能在 macOS 上运行。" >&2
  exit 1
fi

APP_NAME="${APP_NAME:-VoiceInput}"
BUNDLE_ID="${BUNDLE_ID:-com.example.voiceinput.inputmethod}"
CONNECTION_NAME="${CONNECTION_NAME:-com.example.voiceinput.inputmethod_Connection}"
DIST_DIR="${DIST_DIR:-dist}"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
PLIST_FILE="$CONTENTS_DIR/Info.plist"
BIN_NAME="voice-input-macos-ime"
BIN_PATH="target/release/$BIN_NAME"

echo "正在编译系统级 IME 入口"
cargo build -p voice-input-macos --bin "$BIN_NAME" --release

echo "正在组装应用包：$APP_BUNDLE"
rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS_DIR"
cp "$BIN_PATH" "$MACOS_DIR/$APP_NAME"

cat >"$PLIST_FILE" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>zh_CN</string>
  <key>CFBundleExecutable</key>
  <string>$APP_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>InputMethodConnectionName</key>
  <string>$CONNECTION_NAME</string>
  <key>InputMethodServerControllerClass</key>
  <string>VoiceInputInputController</string>
  <key>LSBackgroundOnly</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
EOF

echo "系统级 IME 已打包完成"
echo "应用包：$APP_BUNDLE"
echo "安装方式：将 $APP_BUNDLE 复制到 ~/Library/Input Methods/"
