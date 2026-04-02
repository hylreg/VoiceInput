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
TARGET_BUNDLE_ID="${TARGET_BUNDLE_ID:-com.example.voiceinput.inputmethod}"
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"

echo "== App Bundle =="
echo "APP_BUNDLE=$APP_BUNDLE"
if [[ -d "$APP_BUNDLE" ]]; then
  plutil -p "$APP_BUNDLE/Contents/Info.plist" 2>/dev/null || true
  echo
  echo "xattr:"
  xattr -l "$APP_BUNDLE" 2>/dev/null || echo "(none)"
else
  echo "bundle not found"
fi

echo
echo "== Extension Bundle =="
echo "EXTENSION_BUNDLE=$EXTENSION_BUNDLE"
if [[ -d "$EXTENSION_BUNDLE" ]]; then
  plutil -p "$EXTENSION_BUNDLE/Contents/Info.plist" 2>/dev/null || true
  echo
  echo "xattr:"
  xattr -l "$EXTENSION_BUNDLE" 2>/dev/null || echo "(none)"
else
  echo "bundle not found"
fi

echo
echo "== User HIToolbox =="
defaults read ~/Library/Preferences/com.apple.HIToolbox.plist 2>/dev/null || echo "(no user HIToolbox plist)"

echo
echo "== System HIToolbox =="
defaults read /Library/Preferences/com.apple.HIToolbox.plist 2>/dev/null || echo "(no system HIToolbox plist)"

echo
echo "== TIS Input Sources =="
if command -v swift >/dev/null 2>&1; then
  TARGET_BUNDLE_ID="$TARGET_BUNDLE_ID" swift - <<'SWIFT'
import Carbon.HIToolbox
import Foundation

let target = ProcessInfo.processInfo.environment["TARGET_BUNDLE_ID"] ?? ""
let list = TISCreateInputSourceList(nil, false).takeRetainedValue() as NSArray
var found = false

for case let src as TISInputSource in list {
  var fields: [String] = []

  if let bundle = TISGetInputSourceProperty(src, kTISPropertyBundleID) {
    let bundleID = unsafeBitCast(bundle, to: CFString.self) as String
    fields.append("bundle=\(bundleID)")
    if bundleID == target {
      found = true
      fields.append("TARGET_MATCH")
    }
  }

  if let name = TISGetInputSourceProperty(src, kTISPropertyLocalizedName) {
    let localized = unsafeBitCast(name, to: CFString.self) as String
    fields.append("name=\(localized)")
  }

  if let kind = TISGetInputSourceProperty(src, kTISPropertyInputSourceType) {
    let sourceType = unsafeBitCast(kind, to: CFString.self) as String
    fields.append("type=\(sourceType)")
  }

  print(fields.joined(separator: " | "))
}

if !found, !target.isEmpty {
  print("TARGET_NOT_FOUND: \(target)")
}
SWIFT
else
  echo "swift not found"
fi

echo
echo "== LaunchServices =="
if [[ -x "$LSREGISTER" ]]; then
  "$LSREGISTER" -dump 2>/dev/null | rg -n "VoiceInput|${TARGET_BUNDLE_ID//./\\.}|InputMethodConnectionName|InputMethodServerControllerClass|tsInputMethodCharacterRepertoireKey" || true
else
  echo "lsregister not found"
fi

echo
echo "== mdls =="
if [[ -d "$APP_BUNDLE" ]]; then
  mdls -name kMDItemCFBundleIdentifier -name kMDItemKind -name kMDItemContentType "$APP_BUNDLE" 2>/dev/null || true
fi
