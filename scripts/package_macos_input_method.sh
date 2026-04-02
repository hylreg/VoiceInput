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
