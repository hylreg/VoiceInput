#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" == "Darwin" || "$(uname -s)" == "Linux" ]]; then
  VOICEINPUT_CONFIG_HELPER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  VOICEINPUT_CONFIG_HELPER_REPO_ROOT="$(cd "$VOICEINPUT_CONFIG_HELPER_DIR/.." && pwd)"
else
  return 0 2>/dev/null || exit 0
fi

voiceinput_load_config() {
  local config_file="${VOICEINPUT_CONFIG_FILE:-$VOICEINPUT_CONFIG_HELPER_REPO_ROOT/config/voiceinput.env}"

  if [[ -f "$config_file" ]]; then
    # shellcheck disable=SC1090
    set -a
    source "$config_file"
    set +a
  fi
}

