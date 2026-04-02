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

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "这个一键脚本只能在 Linux 上运行。" >&2
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

backend="ibus"
audio_file=""
run_smoke_after_bootstrap=false
run_live_app_after_bootstrap=true
deploy_args=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --backend)
      if [[ $# -lt 2 ]]; then
        echo "缺少 --backend 的值" >&2
        exit 2
      fi
      backend="$2"
      shift 2
      ;;
    --audio-file)
      if [[ $# -lt 2 ]]; then
        echo "缺少 --audio-file 的值" >&2
        exit 2
      fi
      audio_file="$2"
      run_smoke_after_bootstrap=true
      run_live_app_after_bootstrap=false
      shift 2
      ;;
    --no-launch)
      run_live_app_after_bootstrap=false
      shift
      ;;
    --help|-h)
      cat >&2 <<'EOF'
用法：
  scripts/install_linux_voice_input.sh [--backend ibus|fcitx5] [--audio-file /path/to/audio.wav]

说明：
  - 默认先执行 Linux bootstrap，准备 Python 环境并下载模型
  - 会自动安装 Ubuntu 20.04 常用的 Linux 编译依赖，如 pkg-config、libdbus-1-dev、libibus-1.0-dev
  - 然后自动启动 Linux 常驻托盘版
  - 如果传入 --audio-file，会在准备完成后自动跑一次 Linux smoke
  - --backend 只影响 Linux 常驻版 / smoke 的宿主后端
  - 你也可以额外传入 --install-cuda 等部署参数，它们会原样传给模型部署脚本
EOF
      exit 0
      ;;
    --install-cuda|--skip-existing)
      deploy_args+=("$1")
      shift
      ;;
    --device|--model-id|--local-dir|--revision|--cuda-wheel-index)
      if [[ $# -lt 2 ]]; then
        echo "缺少 $1 的值" >&2
        exit 2
      fi
      deploy_args+=("$1" "$2")
      shift 2
      ;;
    *)
      deploy_args+=("$1")
      shift
      ;;
  esac
done

echo "正在准备本地依赖和模型"
ensure_linux_dev_deps
if [[ ${#deploy_args[@]} -gt 0 ]]; then
  bash scripts/bootstrap.sh "${deploy_args[@]}"
else
  bash scripts/bootstrap.sh
fi

if [[ "$run_smoke_after_bootstrap" == true ]]; then
  echo "正在运行 Linux smoke"
  bash scripts/run_linux_smoke.sh --audio-file "$audio_file" --backend "$backend"
  exit 0
fi

if [[ "$run_live_app_after_bootstrap" == true ]]; then
  echo "正在启动 Linux 常驻托盘版"
  uv run -- "$cargo_bin" run -p voice-input-linux --features ibus --bin voice-input-linux-app -- --backend "$backend"
fi
