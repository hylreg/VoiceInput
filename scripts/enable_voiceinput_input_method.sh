#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "这个脚本只能在 macOS 上运行。" >&2
  exit 1
fi

APP_BUNDLE="${APP_BUNDLE:-$HOME/Library/Input Methods/VoiceInput.app}"
EXTENSION_BUNDLE="${EXTENSION_BUNDLE:-$APP_BUNDLE/Contents/PlugIns/VoiceInput.appex}"
BUNDLE_ID="${BUNDLE_ID:-com.example.voiceinput.inputmethod}"
INPUT_MODE_ID="${INPUT_MODE_ID:-com.example.voiceinput.inputmethod.default}"
INPUT_METHOD_KIND="${INPUT_METHOD_KIND:-Input Mode}"
USER_HIToolbox_PLIST="$HOME/Library/Preferences/com.apple.HIToolbox.plist"
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --app-bundle)
      if [[ $# -lt 2 ]]; then
        echo "缺少 --app-bundle 的值" >&2
        exit 2
      fi
      APP_BUNDLE="$2"
      EXTENSION_BUNDLE="$APP_BUNDLE/Contents/PlugIns/VoiceInput.appex"
      shift 2
      ;;
    --bundle-id)
      if [[ $# -lt 2 ]]; then
        echo "缺少 --bundle-id 的值" >&2
        exit 2
      fi
      BUNDLE_ID="$2"
      shift 2
      ;;
    --input-mode-id)
      if [[ $# -lt 2 ]]; then
        echo "缺少 --input-mode-id 的值" >&2
        exit 2
      fi
      INPUT_MODE_ID="$2"
      shift 2
      ;;
    --help|-h)
      cat <<'EOF'
用法：
  scripts/enable_voiceinput_input_method.sh [--app-bundle /path/to/VoiceInput.app] [--bundle-id ...] [--input-mode-id ...]

说明：
  - 这个脚本只做“启用”和缓存刷新
  - 会注册 pluginkit 插件、写入 HIToolbox 偏好、重启相关缓存进程
  - 不会重新打包，也不会重新复制应用包
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
  echo "请先运行 scripts/reinstall_macos_input_method.sh 或 scripts/dev_install_macos_input_method.sh" >&2
  exit 1
fi

if [[ ! -d "$EXTENSION_BUNDLE" ]]; then
  echo "找不到扩展包：$EXTENSION_BUNDLE" >&2
  echo "请确认这个 app bundle 内含有 VoiceInput.appex" >&2
  exit 1
fi

echo "正在注册 pluginkit 插件：$EXTENSION_BUNDLE"
pluginkit -a "$EXTENSION_BUNDLE" >/dev/null 2>&1 || true
pluginkit -e use -p com.apple.textinputmethod-services -i "$BUNDLE_ID" >/dev/null 2>&1 || true

if [[ ! -f "$USER_HIToolbox_PLIST" ]]; then
  /usr/bin/plutil -create xml1 "$USER_HIToolbox_PLIST"
fi

echo "正在写入用户输入法偏好：$USER_HIToolbox_PLIST"
/usr/bin/plutil -replace AppleEnabledInputSources -json "[{\"InputSourceKind\":\"Keyboard Layout\",\"KeyboardLayout ID\":252,\"KeyboardLayout Name\":\"ABC\"},{\"Bundle ID\":\"com.apple.inputmethod.SCIM\",\"Input Mode\":\"com.apple.inputmethod.SCIM.ITABC\",\"InputSourceKind\":\"Input Mode\"},{\"Bundle ID\":\"com.apple.inputmethod.SCIM\",\"InputSourceKind\":\"Keyboard Input Method\"},{\"Bundle ID\":\"com.apple.CharacterPaletteIM\",\"InputSourceKind\":\"Non Keyboard Input Method\"},{\"Bundle ID\":\"com.apple.inputmethod.ironwood\",\"InputSourceKind\":\"Non Keyboard Input Method\"},{\"Bundle ID\":\"$BUNDLE_ID\",\"Input Mode\":\"$INPUT_MODE_ID\",\"InputSourceKind\":\"$INPUT_METHOD_KIND\"},{\"Bundle ID\":\"$BUNDLE_ID\",\"InputSourceKind\":\"Keyboard Input Method\"}]" "$USER_HIToolbox_PLIST"
/usr/bin/plutil -replace AppleInputSourceHistory -json "[{\"InputSourceKind\":\"Keyboard Layout\",\"KeyboardLayout ID\":252,\"KeyboardLayout Name\":\"ABC\"},{\"Bundle ID\":\"com.apple.inputmethod.SCIM\",\"Input Mode\":\"com.apple.inputmethod.SCIM.ITABC\",\"InputSourceKind\":\"Input Mode\"},{\"Bundle ID\":\"$BUNDLE_ID\",\"Input Mode\":\"$INPUT_MODE_ID\",\"InputSourceKind\":\"$INPUT_METHOD_KIND\"},{\"Bundle ID\":\"$BUNDLE_ID\",\"InputSourceKind\":\"Keyboard Input Method\"}]" "$USER_HIToolbox_PLIST"
/usr/bin/plutil -replace AppleSelectedInputSources -json "[{\"InputSourceKind\":\"Keyboard Layout\",\"KeyboardLayout ID\":252,\"KeyboardLayout Name\":\"ABC\"},{\"Bundle ID\":\"$BUNDLE_ID\",\"Input Mode\":\"$INPUT_MODE_ID\",\"InputSourceKind\":\"$INPUT_METHOD_KIND\"}]" "$USER_HIToolbox_PLIST"

killall cfprefsd TextInputMenuAgent pkd 2>/dev/null || true
sleep 2

if [[ -x "$LSREGISTER" ]]; then
  "$LSREGISTER" -f "$APP_BUNDLE" >/dev/null 2>&1 || true
  "$LSREGISTER" -f "$EXTENSION_BUNDLE" >/dev/null 2>&1 || true
fi

echo "启用完成"
echo "已更新：$APP_BUNDLE"
echo "已更新：$EXTENSION_BUNDLE"
echo "如果系统菜单还没出现，注销并重新登录一次通常就会刷新出来。"
