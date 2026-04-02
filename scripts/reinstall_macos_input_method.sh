#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"
source scripts/macos_input_method_common.sh

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "这个脚本只能在 macOS 上运行。" >&2
  exit 1
fi

APP_BUNDLE="${APP_BUNDLE:-dist/VoiceInput.app}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/Library/Input Methods}"
TARGET_BUNDLE="$INSTALL_DIR/VoiceInput.app"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --app-bundle)
      if [[ $# -lt 2 ]]; then
        echo "缺少 --app-bundle 的值" >&2
        exit 2
      fi
      APP_BUNDLE="$2"
      shift 2
      ;;
    --help|-h)
      cat <<'EOF'
用法：
  scripts/reinstall_macos_input_method.sh [--app-bundle /path/to/VoiceInput.app]

说明：
  - 这个脚本只做调试刷新
  - 默认把 dist/VoiceInput.app 覆盖安装到 ~/Library/Input Methods/
  - 安装完成后会自动执行启用脚本
  - 不会重新 bootstrap、下载模型或重新打包
EOF
      exit 0
      ;;
    *)
      echo "不支持的参数：$1" >&2
      exit 2
      ;;
  esac
done

if [[ ! -d "$APP_BUNDLE" ]]; then
  echo "找不到应用包：$APP_BUNDLE" >&2
  echo "请先运行 scripts/package_macos_input_method.sh" >&2
  exit 1
fi

echo "正在刷新系统输入法安装：$TARGET_BUNDLE"
TARGET_BUNDLE="$(voiceinput_install_bundle "$APP_BUNDLE" "$INSTALL_DIR")"

echo "刷新完成"
echo "已安装到：$TARGET_BUNDLE"
echo "正在启用系统输入法"
voiceinput_enable_bundle "$TARGET_BUNDLE"
