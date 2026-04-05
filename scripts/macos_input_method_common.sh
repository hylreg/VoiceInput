#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  return 0 2>/dev/null || exit 0
fi

VOICEINPUT_LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
VOICEINPUT_HITOOLBOX_PLIST="$HOME/Library/Preferences/com.apple.HIToolbox.plist"

voiceinput_install_bundle() {
  local source_bundle="$1"
  local install_dir="${2:-$HOME/Library/Input Methods}"
  local target_bundle="$install_dir/VoiceInput.app"

  if [[ ! -d "$source_bundle" ]]; then
    echo "找不到应用包：$source_bundle" >&2
    return 1
  fi

  mkdir -p "$install_dir"
  rm -rf "$target_bundle"
  cp -R "$source_bundle" "$target_bundle"
  xattr -cr "$target_bundle"

  if [[ -x "$VOICEINPUT_LSREGISTER" ]]; then
    "$VOICEINPUT_LSREGISTER" -f "$target_bundle" >/dev/null 2>&1 || true
  fi

  printf '%s\n' "$target_bundle"
}

voiceinput_register_tis_bundle() {
  local bundle_path="$1"
  local bundle_id="${2:-com.example.voiceinput.inputmethod}"
  local input_mode_id="${3:-com.example.voiceinput.inputmethod.default}"

  if ! command -v swift >/dev/null 2>&1; then
    echo "找不到 swift，跳过 TIS 注册" >&2
    return 0
  fi

  VOICEINPUT_BUNDLE_PATH="$bundle_path" \
  VOICEINPUT_BUNDLE_ID="$bundle_id" \
  VOICEINPUT_INPUT_MODE_ID="$input_mode_id" \
  swift - <<'SWIFT'
import Carbon.HIToolbox
import Foundation

let env = ProcessInfo.processInfo.environment
let bundlePath = env["VOICEINPUT_BUNDLE_PATH"] ?? ""
let bundleID = env["VOICEINPUT_BUNDLE_ID"] ?? "com.example.voiceinput.inputmethod"
let inputModeID = env["VOICEINPUT_INPUT_MODE_ID"] ?? "com.example.voiceinput.inputmethod.default"

func cfStringValue(_ value: UnsafeMutableRawPointer?) -> String? {
  guard let value else {
    return nil
  }
  return unsafeBitCast(value, to: CFString.self) as String
}

func cfBoolValue(_ value: UnsafeMutableRawPointer?) -> Bool {
  guard let value else {
    return false
  }
  return CFBooleanGetValue(unsafeBitCast(value, to: CFBoolean.self))
}

func inputSourceList(filter: [CFString: Any]) -> [TISInputSource] {
  guard let rawList = TISCreateInputSourceList(filter as CFDictionary, true) else {
    return []
  }
  let list = rawList.takeRetainedValue() as NSArray
  return list as! [TISInputSource]
}

let bundleURL = URL(fileURLWithPath: bundlePath)
let registerStatus = TISRegisterInputSource(bundleURL as CFURL)
print("TISRegisterInputSource(\(bundlePath)) => \(registerStatus)")

var candidates = inputSourceList(filter: [kTISPropertyBundleID: bundleID])
if candidates.isEmpty {
  candidates = inputSourceList(filter: [kTISPropertyInputSourceID: inputModeID])
}

if candidates.isEmpty {
  fputs("未找到可注册的 TIS 输入源：\(bundleID)\n", stderr)
} else {
  for source in candidates {
    let sourceID = cfStringValue(TISGetInputSourceProperty(source, kTISPropertyInputSourceID)) ?? "(unknown)"
    let sourceBundleID = cfStringValue(TISGetInputSourceProperty(source, kTISPropertyBundleID)) ?? "(unknown)"
    let enableStatus = TISEnableInputSource(source)
    let selectCapable = cfBoolValue(TISGetInputSourceProperty(source, kTISPropertyInputSourceIsSelectCapable))
    let selectStatus = selectCapable ? TISSelectInputSource(source) : noErr
    print("TIS source: bundle=\(sourceBundleID) inputSourceID=\(sourceID) enable=\(enableStatus) select=\(selectStatus)")
  }
}
SWIFT
}

voiceinput_enable_bundle() {
  local bundle_path="$1"
  local bundle_id="${2:-com.example.voiceinput.inputmethod}"
  local input_mode_id="${3:-com.example.voiceinput.inputmethod.default}"
  local input_method_kind="${4:-Input Mode}"
  local extension_bundle="${5:-$bundle_path/Contents/PlugIns/VoiceInput.appex}"

  if [[ ! -d "$bundle_path" ]]; then
    echo "找不到应用包：$bundle_path" >&2
    return 1
  fi

  if [[ ! -d "$extension_bundle" ]]; then
    echo "找不到扩展包：$extension_bundle" >&2
    return 1
  fi

  pluginkit -a "$extension_bundle" >/dev/null 2>&1 || true
  pluginkit -e use -p com.apple.textinputmethod-services -i "$bundle_id" >/dev/null 2>&1 || true
  voiceinput_register_tis_bundle "$bundle_path" "$bundle_id" "$input_mode_id" || true
  voiceinput_register_tis_bundle "$extension_bundle" "$bundle_id" "$input_mode_id" || true

  if [[ ! -f "$VOICEINPUT_HITOOLBOX_PLIST" ]]; then
    /usr/bin/plutil -create xml1 "$VOICEINPUT_HITOOLBOX_PLIST"
  fi

  /usr/bin/plutil -replace AppleEnabledInputSources -json "[{\"InputSourceKind\":\"Keyboard Layout\",\"KeyboardLayout ID\":252,\"KeyboardLayout Name\":\"ABC\"},{\"Bundle ID\":\"com.apple.inputmethod.SCIM\",\"Input Mode\":\"com.apple.inputmethod.SCIM.ITABC\",\"InputSourceKind\":\"Input Mode\"},{\"Bundle ID\":\"com.apple.inputmethod.SCIM\",\"InputSourceKind\":\"Keyboard Input Method\"},{\"Bundle ID\":\"com.apple.CharacterPaletteIM\",\"InputSourceKind\":\"Non Keyboard Input Method\"},{\"Bundle ID\":\"com.apple.inputmethod.ironwood\",\"InputSourceKind\":\"Non Keyboard Input Method\"},{\"Bundle ID\":\"$bundle_id\",\"Input Mode\":\"$input_mode_id\",\"InputSourceKind\":\"$input_method_kind\"},{\"Bundle ID\":\"$bundle_id\",\"InputSourceKind\":\"Keyboard Input Method\"}]" "$VOICEINPUT_HITOOLBOX_PLIST"
  /usr/bin/plutil -replace AppleInputSourceHistory -json "[{\"InputSourceKind\":\"Keyboard Layout\",\"KeyboardLayout ID\":252,\"KeyboardLayout Name\":\"ABC\"},{\"Bundle ID\":\"com.apple.inputmethod.SCIM\",\"Input Mode\":\"com.apple.inputmethod.SCIM.ITABC\",\"InputSourceKind\":\"Input Mode\"},{\"Bundle ID\":\"$bundle_id\",\"Input Mode\":\"$input_mode_id\",\"InputSourceKind\":\"$input_method_kind\"},{\"Bundle ID\":\"$bundle_id\",\"InputSourceKind\":\"Keyboard Input Method\"}]" "$VOICEINPUT_HITOOLBOX_PLIST"
  /usr/bin/plutil -replace AppleSelectedInputSources -json "[{\"InputSourceKind\":\"Keyboard Layout\",\"KeyboardLayout ID\":252,\"KeyboardLayout Name\":\"ABC\"},{\"Bundle ID\":\"$bundle_id\",\"Input Mode\":\"$input_mode_id\",\"InputSourceKind\":\"$input_method_kind\"}]" "$VOICEINPUT_HITOOLBOX_PLIST"

  killall cfprefsd TextInputMenuAgent TextInputSwitcher pkd 2>/dev/null || true
  sleep 2
  launchctl kickstart -k "gui/$UID/com.apple.TextInputSwitcher" >/dev/null 2>&1 || true
  launchctl kickstart -k "gui/$UID/com.apple.TextInputMenuAgent" >/dev/null 2>&1 || true
  launchctl kickstart -k "gui/$UID/com.apple.pluginkit.pkd" >/dev/null 2>&1 || true

  if [[ -x "$VOICEINPUT_LSREGISTER" ]]; then
    "$VOICEINPUT_LSREGISTER" -f "$bundle_path" >/dev/null 2>&1 || true
    "$VOICEINPUT_LSREGISTER" -f "$extension_bundle" >/dev/null 2>&1 || true
  fi
}

voiceinput_sync_bundle() {
  local source_bundle="$1"
  local install_dir="${2:-$HOME/Library/Input Methods}"
  local bundle_id="${3:-com.example.voiceinput.inputmethod}"
  local input_mode_id="${4:-com.example.voiceinput.inputmethod.default}"
  local input_method_kind="${5:-Input Mode}"

  local target_bundle
  target_bundle="$(voiceinput_install_bundle "$source_bundle" "$install_dir")"
  voiceinput_enable_bundle "$target_bundle" "$bundle_id" "$input_mode_id" "$input_method_kind"
  printf '%s\n' "$target_bundle"
}
