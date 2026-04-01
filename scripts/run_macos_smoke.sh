#!/usr/bin/env bash
set -euo pipefail

if ! command -v uv >/dev/null 2>&1; then
  echo "需要先安装 uv。安装说明：https://docs.astral.sh/uv/" >&2
  exit 1
fi

if [[ $# -lt 2 || $1 != "--audio-file" ]]; then
  echo "用法：scripts/run_macos_smoke.sh --audio-file /path/to/audio.wav" >&2
  exit 2
fi

uv run -- cargo run -p voice-input-macos -- "$@"
