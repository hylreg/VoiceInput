#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ensure_cargo() {
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return 0
  fi

  if command -v rustup >/dev/null 2>&1; then
    local cargo_path
    cargo_path="$(rustup which cargo 2>/dev/null || true)"
    if [[ -n "$cargo_path" ]]; then
      export PATH="$(cd "$(dirname "$cargo_path")" && pwd):$PATH"
    fi
  fi

  if ! command -v cargo >/dev/null 2>&1; then
    echo "未找到 cargo。请先安装 Rust 工具链，并确保 rustup 已完成初始化。" >&2
    exit 1
  fi
}

ensure_uv() {
  if command -v uv >/dev/null 2>&1; then
    return 0
  fi

  echo "未找到 uv。请先安装 uv：https://docs.astral.sh/uv/" >&2
  exit 1
}

cd "$REPO_ROOT"

ensure_cargo
ensure_uv

deploy_args=()
smoke_audio_file=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --audio-file)
      if [[ $# -lt 2 ]]; then
        echo "缺少 --audio-file 的值" >&2
        exit 2
      fi
      smoke_audio_file="$2"
      shift 2
      ;;
    --help|-h)
      cat >&2 <<'EOF'
用法：
  scripts/bootstrap.sh [部署参数...] [--audio-file /path/to/audio.wav]

说明：
  - 未传 --audio-file 时，只执行 Python 环境和模型部署
  - 传入 --audio-file 时，会在部署完成后自动运行 macOS smoke
  - 部署参数会原样传给 deploy_funasr_model.py

常用部署参数：
  --skip-existing
  --install-cuda
  --device auto|cpu|cuda|mps
EOF
      exit 0
      ;;
    *)
      deploy_args+=("$1")
      shift
      ;;
  esac
done

echo "正在创建或更新 Python 虚拟环境：.venv"
uv venv .venv

echo "正在安装 Python 依赖"
uv pip install -r scripts/requirements-asr.txt

echo "正在部署本地 Fun-ASR 模型"
uv run -- python scripts/deploy_funasr_model.py --skip-existing "${deploy_args[@]}"

if [[ -n "$smoke_audio_file" ]]; then
  echo "正在运行 macOS smoke"
  uv run -- cargo run -p voice-input-macos -- --audio-file "$smoke_audio_file"
fi

echo "一键部署完成"
echo "Rust：$(cargo --version)"
echo "uv：$(uv --version)"
