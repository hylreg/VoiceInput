#!/usr/bin/env bash
set -euo pipefail

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

  local lsregister="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
  if [[ -x "$lsregister" ]]; then
    "$lsregister" -f "$target_bundle" >/dev/null 2>&1 || true
  fi

  printf '%s\n' "$target_bundle"
}

voiceinput_enable_bundle() {
  local bundle_path="$1"

  if [[ ! -d "$bundle_path" ]]; then
    echo "找不到应用包：$bundle_path" >&2
    return 1
  fi

  bash scripts/enable_voiceinput_input_method.sh --app-bundle "$bundle_path"
}
