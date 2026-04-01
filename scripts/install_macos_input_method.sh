#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "这个一键安装脚本只能在 macOS 上运行。" >&2
  exit 1
fi

audio_file=""
run_smoke_after_install=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --audio-file)
      if [[ $# -lt 2 ]]; then
        echo "缺少 --audio-file 的值" >&2
        exit 2
      fi
      audio_file="$2"
      run_smoke_after_install=true
      shift 2
      ;;
    --skip-smoke)
      run_smoke_after_install=false
      audio_file=""
      shift
      ;;
    --help|-h)
      cat >&2 <<'EOF'
用法：
  scripts/install_macos_input_method.sh [--audio-file /path/to/audio.wav]

说明：
  - 先创建 Python 环境并下载本地模型
  - 再打包系统级 macOS 输入法应用
  - 最后复制到 ~/Library/Input Methods/
  - 如果传入 --audio-file，会在安装后自动运行一次 smoke 验证
EOF
      exit 0
      ;;
    *)
      echo "不支持的参数：$1" >&2
      exit 2
      ;;
  esac
done

echo "正在准备本地依赖和模型"
bash scripts/bootstrap.sh

echo "正在打包系统级输入法应用"
bash scripts/package_macos_input_method.sh

INSTALL_DIR="$HOME/Library/Input Methods"
APP_BUNDLE="dist/VoiceInput.app"
TARGET_BUNDLE="$INSTALL_DIR/VoiceInput.app"

echo "正在安装到系统输入法目录：$TARGET_BUNDLE"
mkdir -p "$INSTALL_DIR"
rm -rf "$TARGET_BUNDLE"
cp -R "$APP_BUNDLE" "$TARGET_BUNDLE"

echo "安装完成"
echo "已安装到：$TARGET_BUNDLE"
echo "请重新登录或重启输入法服务，然后在系统输入法列表中启用 VoiceInput"
echo "首次运行前建议授予“麦克风”和“辅助功能”权限"

if [[ "$run_smoke_after_install" == true ]]; then
  echo "正在运行 smoke 验证"
  bash scripts/run_macos_smoke.sh --audio-file "$audio_file"
fi
