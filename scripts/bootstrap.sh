#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

configure_rust_mirror() {
  export RUSTUP_DIST_SERVER="${RUSTUP_DIST_SERVER:-https://mirrors.aliyun.com/rustup}"
  export RUSTUP_UPDATE_ROOT="${RUSTUP_UPDATE_ROOT:-https://mirrors.aliyun.com/rustup/rustup}"
}

run_without_proxy() {
  env \
    -u http_proxy -u https_proxy -u all_proxy \
    -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
    "$@"
}

clear_proxy_env() {
  unset http_proxy https_proxy all_proxy HTTP_PROXY HTTPS_PROXY ALL_PROXY
}

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

install_rustup() {
  configure_rust_mirror

  if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1 && ! command -v python3 >/dev/null 2>&1; then
    echo "未找到 curl、wget 或 python3，无法自动安装 Rust 工具链。" >&2
    exit 1
  fi

  local rustup_init="${TMPDIR:-/tmp}/rustup-init.sh"
  if command -v curl >/dev/null 2>&1; then
    run_without_proxy curl -fsSL --retry 3 --noproxy '*' https://sh.rustup.rs -o "$rustup_init"
  else
    if command -v wget >/dev/null 2>&1; then
      run_without_proxy wget -qO "$rustup_init" --no-proxy https://sh.rustup.rs
    else
      run_without_proxy python3 - "$rustup_init" https://sh.rustup.rs <<'PY'
import sys
import urllib.request

out_path = sys.argv[1]
url = sys.argv[2]
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
with opener.open(url) as response, open(out_path, "wb") as output:
    output.write(response.read())
PY
    fi
  fi

  chmod +x "$rustup_init"
  run_without_proxy sh "$rustup_init" -y --default-toolchain stable --profile minimal

  refresh_cargo_path
}

ensure_cargo() {
  refresh_cargo_path
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return 0
  fi

  if command -v rustup >/dev/null 2>&1; then
    echo "检测到 rustup，但 cargo/rustc 未就绪，正在尝试修复环境..." >&2
    refresh_cargo_path
  else
    echo "未找到 Rust 工具链，正在使用阿里云源自动安装 rustup..." >&2
    install_rustup
  fi

  if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    echo "未能自动准备好 cargo/rustc。请手动检查 rustup 安装是否成功。" >&2
    exit 1
  fi
}

ensure_rustfmt() {
  if ! command -v rustup >/dev/null 2>&1; then
    return 0
  fi

  if rustup component list --installed | grep -q '^rustfmt '; then
    return 0
  fi

  echo "正在安装 rustfmt 组件"
  run_without_proxy rustup component add rustfmt
}

ensure_uv() {
  if command -v uv >/dev/null 2>&1; then
    return 0
  fi

  echo "未找到 uv。请先安装 uv：https://docs.astral.sh/uv/" >&2
  exit 1
}

cd "$REPO_ROOT"

export UV_DEFAULT_INDEX="${UV_DEFAULT_INDEX:-https://mirrors.aliyun.com/pypi/simple/}"
clear_proxy_env
configure_rust_mirror

ensure_cargo
ensure_rustfmt
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
UV_VENV_CLEAR=1 uv venv .venv --python "$(command -v python3.12)"

echo "正在安装模型下载依赖"
uv pip install -r scripts/requirements-asr-base.txt

echo "正在安装 ASR 运行时依赖"
uv pip install -r scripts/requirements-asr-runtime.txt

echo "正在部署本地 Fun-ASR 模型"
if [[ ${#deploy_args[@]} -gt 0 ]]; then
  uv run -- python scripts/deploy_funasr_model.py --skip-existing "${deploy_args[@]}"
else
  uv run -- python scripts/deploy_funasr_model.py --skip-existing
fi

if [[ -n "$smoke_audio_file" ]]; then
  echo "正在运行 macOS smoke"
  uv run -- cargo run -p voice-input-macos -- --audio-file "$smoke_audio_file"
fi

echo "一键部署完成"
echo "Rust：$(cargo --version)"
echo "uv：$(uv --version)"
