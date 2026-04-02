#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

configure_rust_mirror() {
  export RUSTUP_DIST_SERVER="${RUSTUP_DIST_SERVER:-https://mirrors.aliyun.com/rustup}"
  export RUSTUP_UPDATE_ROOT="${RUSTUP_UPDATE_ROOT:-https://mirrors.aliyun.com/rustup/rustup}"
}

run_without_proxy() {
  env \
    -u http_proxy -u https_proxy -u all_proxy \
    -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
    "$@"
}

refresh_cargo_path() {
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return 0
  fi

  if [[ -f "${HOME}/.cargo/env" ]]; then
    # shellcheck disable=SC1090
    source "${HOME}/.cargo/env"
  fi

  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return 0
  fi

  if command -v rustup >/dev/null 2>&1; then
    local cargo_path
    cargo_path="$(rustup which cargo 2>/dev/null || true)"
    if [[ -n "${cargo_path:-}" ]]; then
      export PATH="$(cd "$(dirname "$cargo_path")" && pwd):$PATH"
    fi
  fi
}

install_rustup() {
  configure_rust_mirror

  if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1 && ! command -v python3 >/dev/null 2>&1; then
    echo "未找到 curl、wget 或 python3，无法自动安装 Rust 工具链。" >&2
    exit 1
  fi

  local rustup_init="${TMPDIR:-/tmp}/rustup-init.sh"
  if command -v curl >/dev/null 2>&1; then
    run_without_proxy curl -fsSL --retry 3 --noproxy '*' https://sh.rustup.rs -o "$rustup_init"
  else
    if command -v wget >/dev/null 2>&1; then
      run_without_proxy wget -qO "$rustup_init" --no-proxy https://sh.rustup.rs
    else
      run_without_proxy python3 - "$rustup_init" https://sh.rustup.rs <<'PY'
import sys
import urllib.request

out_path = sys.argv[1]
url = sys.argv[2]
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
with opener.open(url) as response, open(out_path, "wb") as output:
    output.write(response.read())
PY
    fi
  fi

  chmod +x "$rustup_init"
  run_without_proxy sh "$rustup_init" -y --default-toolchain stable --profile minimal

  refresh_cargo_path
}

ensure_cargo() {
  refresh_cargo_path
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return 0
  fi

  if command -v rustup >/dev/null 2>&1; then
    echo "检测到 rustup，但 cargo/rustc 未就绪，正在尝试修复环境..." >&2
    refresh_cargo_path
  else
    echo "未找到 Rust 工具链，正在使用阿里云源自动安装 rustup..." >&2
    install_rustup
  fi

  if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    echo "未能自动准备好 cargo/rustc。请手动检查 rustup 安装是否成功。" >&2
    exit 1
  fi
}

ensure_rustfmt() {
  if ! command -v rustup >/dev/null 2>&1; then
    return 0
  fi

  if rustup component list --installed | grep -q '^rustfmt '; then
    return 0
  fi

  echo "正在安装 rustfmt 组件"
  run_without_proxy rustup component add rustfmt
}

cd "$REPO_ROOT"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "这个脚本只能在 macOS 上运行。" >&2
  exit 1
fi

configure_rust_mirror
ensure_cargo
ensure_rustfmt

APP_NAME="${APP_NAME:-VoiceInput}"
CONTAINER_BUNDLE_ID="${CONTAINER_BUNDLE_ID:-com.example.voiceinput.container}"
INPUT_METHOD_BUNDLE_ID="${INPUT_METHOD_BUNDLE_ID:-com.example.voiceinput.inputmethod}"
CONNECTION_NAME="${CONNECTION_NAME:-com.example.voiceinput.inputmethod_Connection}"
INPUT_MODE_ID="${INPUT_MODE_ID:-com.example.voiceinput.inputmethod.default}"
INPUT_MODE_DISPLAY_NAME="${INPUT_MODE_DISPLAY_NAME:-VoiceInput}"
INPUT_MODE_SHORT_NAME="${INPUT_MODE_SHORT_NAME:-VoiceInput}"
DIST_DIR="${DIST_DIR:-dist}"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
PLUGINS_DIR="$CONTENTS_DIR/PlugIns"
EXTENSION_NAME="${EXTENSION_NAME:-VoiceInput.appex}"
EXTENSION_BUNDLE="$PLUGINS_DIR/$EXTENSION_NAME"
EXTENSION_CONTENTS_DIR="$EXTENSION_BUNDLE/Contents"
EXTENSION_MACOS_DIR="$EXTENSION_CONTENTS_DIR/MacOS"
EXTENSION_RESOURCES_DIR="$EXTENSION_CONTENTS_DIR/Resources"
PLIST_FILE="$CONTENTS_DIR/Info.plist"
EXTENSION_ENTITLEMENTS="${TMPDIR:-/tmp}/voiceinput-macos-extension.entitlements"
APP_BIN_NAME="voice-input-macos-app"
IME_BIN_NAME="voice-input-macos-ime"
APP_BIN_PATH="target/release/$APP_BIN_NAME"
IME_BIN_PATH="target/release/$IME_BIN_NAME"
ICON_SOURCE="/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericApplicationIcon.icns"
ICON_NAME="VoiceInput.icns"
PKGINFO_APP="APPL????"
PKGINFO_EXTENSION="XPC!????"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"

echo "正在编译 macOS 容器和系统级 IME 入口"
cargo build -p voice-input-macos --bin "$APP_BIN_NAME" --bin "$IME_BIN_NAME" --release

echo "正在组装应用包：$APP_BUNDLE"
rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS_DIR"
mkdir -p "$RESOURCES_DIR"
mkdir -p "$PLUGINS_DIR"
mkdir -p "$EXTENSION_MACOS_DIR"
mkdir -p "$EXTENSION_RESOURCES_DIR"
cp "$APP_BIN_PATH" "$MACOS_DIR/$APP_NAME"
cp "$IME_BIN_PATH" "$EXTENSION_MACOS_DIR/VoiceInputInputMethod"
cp "$ICON_SOURCE" "$RESOURCES_DIR/$ICON_NAME"
cp "$ICON_SOURCE" "$EXTENSION_RESOURCES_DIR/$ICON_NAME"

cat >"$CONTENTS_DIR/PkgInfo" <<EOF
$PKGINFO_APP
EOF

cat >"$CONTENTS_DIR/version.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
</dict>
</plist>
EOF

cat >"$PLIST_FILE" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>zh_CN</string>
  <key>CFBundleDisplayName</key>
  <string>$APP_NAME</string>
  <key>CFBundleExecutable</key>
  <string>$APP_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$CONTAINER_BUNDLE_ID</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleSupportedPlatforms</key>
  <array>
    <string>MacOSX</string>
  </array>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>CFBundleSignature</key>
  <string>????</string>
  <key>LSUIElement</key>
  <true/>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
  <key>LSBackgroundOnly</key>
  <true/>
  <key>NSSupportsSuddenTermination</key>
  <false/>
</dict>
</plist>
EOF

cat >"$EXTENSION_CONTENTS_DIR/PkgInfo" <<EOF
$PKGINFO_EXTENSION
EOF

cat >"$EXTENSION_CONTENTS_DIR/version.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
</dict>
</plist>
EOF

cat >"$EXTENSION_ENTITLEMENTS" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.app-sandbox</key>
  <true/>
  <key>com.apple.security.network.client</key>
  <true/>
  <key>com.apple.security.temporary-exception.mach-register.global-name</key>
  <array>
    <string>$CONNECTION_NAME</string>
  </array>
</dict>
</plist>
EOF

cat >"$EXTENSION_CONTENTS_DIR/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>zh_CN</string>
  <key>CFBundleDisplayName</key>
  <string>$APP_NAME</string>
  <key>CFBundleExecutable</key>
  <string>VoiceInputInputMethod</string>
  <key>CFBundleGetInfoString</key>
  <string>VoiceInput Input Method</string>
  <key>CFBundleIdentifier</key>
  <string>$INPUT_METHOD_BUNDLE_ID</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>XPC!</string>
  <key>CFBundleSupportedPlatforms</key>
  <array>
    <string>MacOSX</string>
  </array>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleSignature</key>
  <string>????</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>InputMethodConnectionName</key>
  <string>$CONNECTION_NAME</string>
  <key>IMKExtensionDelegateClass</key>
  <string>VoiceInputExtensionDelegate</string>
  <key>InputMethodServerControllerClass</key>
  <string>VoiceInputInputController</string>
  <key>InputMethodServerDelegateClass</key>
  <string>VoiceInputInputController</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSSupportsSuddenTermination</key>
  <false/>
  <key>NSExtension</key>
  <dict>
    <key>NSExtensionPointIdentifier</key>
    <string>com.apple.textinputmethod-services</string>
  </dict>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
  <key>ComponentInputModeDict</key>
  <dict>
    <key>tsInputModeListKey</key>
    <dict>
      <key>$INPUT_MODE_ID</key>
      <dict>
        <key>TISDoubleSpaceSubstitution</key>
        <string>。</string>
        <key>TISIconLabels</key>
        <dict>
          <key>Primary</key>
          <string>$INPUT_MODE_SHORT_NAME</string>
        </dict>
        <key>TISInputSourceID</key>
        <string>$INPUT_MODE_ID</string>
        <key>TISIntendedLanguage</key>
        <string>zh-Hans</string>
        <key>tsInputModeCharacterRepertoireKey</key>
        <array>
          <string>Hans</string>
        </array>
        <key>tsInputModeIsVisibleKey</key>
        <true/>
        <key>tsInputModeMenuIconFileKey</key>
        <string>$ICON_NAME</string>
        <key>tsInputModePaletteIconFileKey</key>
        <string>$ICON_NAME</string>
        <key>tsInputModePrimaryInScriptKey</key>
        <false/>
        <key>tsInputModeScriptKey</key>
        <string>smSimpChinese</string>
      </dict>
    </dict>
    <key>tsVisibleInputModeOrderedArrayKey</key>
    <array>
      <string>$INPUT_MODE_ID</string>
    </array>
  </dict>
  <key>TICapsLockLanguageSwitchCapable</key>
  <true/>
  <key>TISIconIsTemplate</key>
  <true/>
  <key>TISInputSourceID</key>
  <string>$INPUT_METHOD_BUNDLE_ID</string>
  <key>TISIntendedLanguage</key>
  <string>zh-Hans</string>
  <key>tsInputMethodCharacterRepertoireKey</key>
  <array>
    <string>Hans</string>
  </array>
  <key>tsInputMethodIconFileKey</key>
  <string>$ICON_NAME</string>
</dict>
</plist>
EOF

echo "正在签名系统级 IME 扩展"
codesign --force --sign "$SIGN_IDENTITY" --identifier "$INPUT_METHOD_BUNDLE_ID" --entitlements "$EXTENSION_ENTITLEMENTS" --timestamp=none "$EXTENSION_BUNDLE"

echo "正在签名容器应用"
codesign --force --deep --sign "$SIGN_IDENTITY" --identifier "$CONTAINER_BUNDLE_ID" --entitlements "$EXTENSION_ENTITLEMENTS" --timestamp=none "$APP_BUNDLE"

xattr -cr "$APP_BUNDLE"

echo "系统级 IME 已打包完成"
echo "应用包：$APP_BUNDLE"
echo "安装方式：将 $APP_BUNDLE 复制到 ~/Library/Input Methods/"
