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
  scripts/dev_install_macos_input_method.sh [--app-bundle /path/to/VoiceInput.app]

说明：
  - 这个脚本用于开发调试
  - 会先运行 scripts/package_macos_input_method.sh
  - 再执行打包后的安装和启用流程
  - 不会重新 bootstrap、下载模型或跑 smoke
EOF
      exit 0
      ;;
    *)
      echo "不支持的参数：$1" >&2
      exit 2
      ;;
  esac
done

echo "正在打包 macOS 输入法"
bash scripts/package_macos_input_method.sh

echo "正在刷新系统输入法安装"
TARGET_BUNDLE="$(voiceinput_install_bundle "$APP_BUNDLE" "$HOME/Library/Input Methods")"

echo "正在启用系统输入法"
voiceinput_enable_bundle "$TARGET_BUNDLE"

echo "开发安装完成"
