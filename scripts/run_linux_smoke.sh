#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

refresh_cargo_path() {
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return 0
  fi

  if [[ -f "${HOME}/.cargo/env" ]]; then
    # shellcheck disable=SC1090
    source "${HOME}/.cargo/env"
  fi

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
}

ensure_linux_dev_deps() {
  local required_packages=(
    pkg-config
    libdbus-1-dev
    libibus-1.0-dev
    libx11-dev
    libasound2-dev
    portaudio19-dev
  )
  local missing_packages=()

  for package in "${required_packages[@]}"; do
    if ! dpkg -s "$package" >/dev/null 2>&1; then
      missing_packages+=("$package")
    fi
  done

  if ((${#missing_packages[@]} == 0)); then
    return 0
  fi

  if ! command -v apt-get >/dev/null 2>&1; then
    echo "缺少 Linux 依赖：${missing_packages[*]}" >&2
    echo "当前系统没有 apt-get，无法自动安装这些包。" >&2
    exit 2
  fi

  local apt_cmd
  if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
    apt_cmd=(apt-get)
  elif command -v sudo >/dev/null 2>&1; then
    apt_cmd=(sudo apt-get)
  else
    echo "缺少 Linux 依赖：${missing_packages[*]}" >&2
    echo "需要 root 权限或 sudo 才能自动安装这些包。" >&2
    exit 2
  fi

  echo "正在自动安装 Linux 依赖：${missing_packages[*]}"
  "${apt_cmd[@]}" update
  DEBIAN_FRONTEND=noninteractive "${apt_cmd[@]}" install -y "${missing_packages[@]}"
}

cd "$REPO_ROOT"

if ! command -v uv >/dev/null 2>&1; then
  echo "需要先安装 uv。安装说明：https://docs.astral.sh/uv/" >&2
  exit 1
fi

refresh_cargo_path
cargo_bin="$(command -v cargo || true)"
if [[ -z "$cargo_bin" && -x "${HOME}/.cargo/bin/cargo" ]]; then
  cargo_bin="${HOME}/.cargo/bin/cargo"
fi
if [[ -z "$cargo_bin" ]]; then
  echo "未找到 cargo。请先安装 Rust 工具链。" >&2
  exit 1
fi

ensure_ibus_dev_deps
ensure_linux_dev_deps

if [[ $# -lt 2 || "$1" != "--audio-file" ]]; then
  cat >&2 <<'EOF'
用法：
  scripts/run_linux_smoke.sh --audio-file /path/to/audio.wav [--backend ibus|fcitx5]

说明：
  - 默认使用 IBus
  - 需要先准备好 Python ASR 环境和本地模型
  - 如果当前 crate 没有启用 IBus feature，请改用：
    cargo run -p voice-input-linux --features ibus -- --audio-file ...
EOF
  exit 2
fi

uv run -- "$cargo_bin" run -p voice-input-linux --features ibus -- "$@"
