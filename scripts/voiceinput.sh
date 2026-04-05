#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# shellcheck disable=SC1090
source "$SCRIPT_DIR/voiceinput_config.sh"
voiceinput_load_config

voiceinput_configure_rust_mirror() {
  export RUSTUP_DIST_SERVER="${RUSTUP_DIST_SERVER:-https://mirrors.aliyun.com/rustup}"
  export RUSTUP_UPDATE_ROOT="${RUSTUP_UPDATE_ROOT:-https://mirrors.aliyun.com/rustup/rustup}"
}

voiceinput_run_without_proxy() {
  env \
    -u http_proxy -u https_proxy -u all_proxy \
    -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
    "$@"
}

voiceinput_clear_proxy_env() {
  unset http_proxy https_proxy all_proxy HTTP_PROXY HTTPS_PROXY ALL_PROXY
}

voiceinput_refresh_cargo_path() {
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

voiceinput_ensure_cargo() {
  voiceinput_refresh_cargo_path
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return 0
  fi

  if command -v rustup >/dev/null 2>&1; then
    echo "检测到 rustup，但 cargo/rustc 未就绪，正在尝试修复环境..." >&2
    voiceinput_refresh_cargo_path
  else
    echo "未找到 Rust 工具链，正在使用阿里云源自动安装 rustup..." >&2
    voiceinput_install_rustup
  fi

  if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    echo "未能自动准备好 cargo/rustc。请手动检查 rustup 安装是否成功。" >&2
    exit 1
  fi
}

voiceinput_install_rustup() {
  voiceinput_configure_rust_mirror

  if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1 && ! command -v python3 >/dev/null 2>&1; then
    echo "未找到 curl、wget 或 python3，无法自动安装 Rust 工具链。" >&2
    exit 1
  fi

  local rustup_init="${TMPDIR:-/tmp}/rustup-init.sh"
  if command -v curl >/dev/null 2>&1; then
    voiceinput_run_without_proxy curl -fsSL --retry 3 --noproxy '*' https://sh.rustup.rs -o "$rustup_init"
  else
    if command -v wget >/dev/null 2>&1; then
      voiceinput_run_without_proxy wget -qO "$rustup_init" --no-proxy https://sh.rustup.rs
    else
      voiceinput_run_without_proxy python3 - "$rustup_init" https://sh.rustup.rs <<'PY'
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
  voiceinput_run_without_proxy sh "$rustup_init" -y --default-toolchain stable --profile minimal

  voiceinput_refresh_cargo_path
}

voiceinput_ensure_rustfmt() {
  if ! command -v rustup >/dev/null 2>&1; then
    return 0
  fi

  if rustup component list --installed | grep -q '^rustfmt '; then
    return 0
  fi

  echo "正在安装 rustfmt 组件"
  voiceinput_run_without_proxy rustup component add rustfmt
}

voiceinput_ensure_uv() {
  if command -v uv >/dev/null 2>&1; then
    return 0
  fi

  echo "需要先安装 uv。安装说明：https://docs.astral.sh/uv/" >&2
  exit 1
}

voiceinput_ensure_linux_dev_deps() {
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

voiceinput_normalize_model_choice() {
  local choice="${1:-}"
  choice="$(printf '%s' "$choice" | tr '[:upper:]' '[:lower:]')"

  case "$choice" in
    funasr|fun)
      printf '%s\n' "funasr"
      ;;
    qwen|qwen3|qwen-asr)
      printf '%s\n' "qwen"
      ;;
    qwen-0.6b|qwen0.6b|qwen06|qwen3-0.6b|qwen3-asr-0.6b)
      printf '%s\n' "qwen-0.6b"
      ;;
    *)
      return 1
      ;;
  esac
}

voiceinput_model_backend_for_choice() {
  local choice
  choice="$(voiceinput_normalize_model_choice "${1:-}")" || return 1

  case "$choice" in
    funasr)
      printf '%s\n' "funasr"
      ;;
    qwen|qwen-0.6b)
      printf '%s\n' "qwen"
      ;;
    *)
      return 1
      ;;
  esac
}

voiceinput_model_id_for_choice() {
  local choice
  choice="$(voiceinput_normalize_model_choice "${1:-}")" || return 1

  case "$choice" in
    funasr)
      printf '%s\n' "FunAudioLLM/Fun-ASR-Nano-2512"
      ;;
    qwen)
      printf '%s\n' "Qwen/Qwen3-ASR-1.7B"
      ;;
    qwen-0.6b)
      printf '%s\n' "Qwen/Qwen3-ASR-0.6B"
      ;;
    *)
      return 1
      ;;
  esac
}

voiceinput_model_source_url_for_choice() {
  local choice
  choice="$(voiceinput_normalize_model_choice "${1:-}")" || return 1

  case "$choice" in
    funasr)
      printf '%s\n' "https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512"
      ;;
    qwen|qwen-0.6b)
      printf '%s\n' "https://www.modelscope.cn/collections/Qwen/Qwen3-ASR"
      ;;
    *)
      return 1
      ;;
  esac
}

voiceinput_model_local_dir_for_choice() {
  local choice
  choice="$(voiceinput_normalize_model_choice "${1:-}")" || return 1

  case "$choice" in
    funasr)
      printf '%s\n' "./models/FunAudioLLM/Fun-ASR-Nano-2512"
      ;;
    qwen)
      printf '%s\n' "./models/Qwen/Qwen3-ASR-1.7B"
      ;;
    qwen-0.6b)
      printf '%s\n' "./models/Qwen/Qwen3-ASR-0.6B"
      ;;
    *)
      return 1
      ;;
  esac
}

voiceinput_apply_model_choice_env() {
  local choice
  choice="$(voiceinput_normalize_model_choice "${1:-}")" || return 1

  export VOICEINPUT_ASR_MODEL="$choice"
  export VOICEINPUT_ASR_BACKEND="$(voiceinput_model_backend_for_choice "$choice")"
  export VOICEINPUT_ASR_MODEL_ID="$(voiceinput_model_id_for_choice "$choice")"
  export VOICEINPUT_ASR_SOURCE_URL="$(voiceinput_model_source_url_for_choice "$choice")"
  export VOICEINPUT_ASR_MODEL_DIR="$(voiceinput_model_local_dir_for_choice "$choice")"
}

voiceinput_config_file_path() {
  printf '%s\n' "${VOICEINPUT_CONFIG_FILE:-$REPO_ROOT/config/voiceinput.env}"
}

voiceinput_write_model_config() {
  local model="$1"
  local config_file="${2:-$(voiceinput_config_file_path)}"
  local normalized_model
  normalized_model="$(voiceinput_normalize_model_choice "$model")" || return 1

  local tmp_file
  tmp_file="$(mktemp "${config_file}.XXXXXX")"

  case "$normalized_model" in
    funasr)
      cat >"$tmp_file" <<'EOF'
# VoiceInput shared configuration template.
# Shell scripts source this file before deploying models or launching runtime.
# Use `scripts/voiceinput.sh model qwen`, `scripts/voiceinput.sh model qwen-0.6b`
# or `scripts/voiceinput.sh model funasr` to rewrite this file with a selected
# default model.
# Command-line arguments and explicit environment variables still override it.
#
# Keep one preset block uncommented below if you want a local default.

## -------------------------------------------------------------------
## FunASR preset
## -------------------------------------------------------------------
export VOICEINPUT_ASR_MODEL="funasr"
export VOICEINPUT_ASR_BACKEND="funasr"
export VOICEINPUT_ASR_MODEL_ID="FunAudioLLM/Fun-ASR-Nano-2512"
export VOICEINPUT_ASR_SOURCE_URL="https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512"
export VOICEINPUT_ASR_MODEL_DIR="./models/FunAudioLLM/Fun-ASR-Nano-2512"
export VOICEINPUT_ASR_REMOTE_CODE="./models/FunAudioLLM/Fun-ASR-Nano-2512/model.py"
export VOICEINPUT_ASR_DEVICE="auto"
export VOICEINPUT_ASR_LANGUAGE="中文"
export VOICEINPUT_ASR_ITN="true"
export VOICEINPUT_ASR_HOTWORDS=""

## -------------------------------------------------------------------
## Qwen preset
## -------------------------------------------------------------------
# export VOICEINPUT_ASR_MODEL="qwen"
# export VOICEINPUT_ASR_BACKEND="qwen"
# export VOICEINPUT_ASR_MODEL_ID="Qwen/Qwen3-ASR-1.7B"
# export VOICEINPUT_ASR_SOURCE_URL="https://www.modelscope.cn/collections/Qwen/Qwen3-ASR"
# export VOICEINPUT_ASR_MODEL_DIR="./models/Qwen/Qwen3-ASR-1.7B"
# export VOICEINPUT_ASR_DEVICE="auto"
# export VOICEINPUT_ASR_LANGUAGE="中文"
# export VOICEINPUT_ASR_ITN="true"
# export VOICEINPUT_ASR_HOTWORDS=""

## -------------------------------------------------------------------
## Qwen 0.6B preset
## -------------------------------------------------------------------
# export VOICEINPUT_ASR_MODEL="qwen-0.6b"
# export VOICEINPUT_ASR_BACKEND="qwen"
# export VOICEINPUT_ASR_MODEL_ID="Qwen/Qwen3-ASR-0.6B"
# export VOICEINPUT_ASR_SOURCE_URL="https://www.modelscope.cn/collections/Qwen/Qwen3-ASR"
# export VOICEINPUT_ASR_MODEL_DIR="./models/Qwen/Qwen3-ASR-0.6B"
# export VOICEINPUT_ASR_DEVICE="auto"
# export VOICEINPUT_ASR_LANGUAGE="中文"
# export VOICEINPUT_ASR_ITN="true"
# export VOICEINPUT_ASR_HOTWORDS=""
EOF
      ;;
    qwen-0.6b)
      cat >"$tmp_file" <<'EOF'
# VoiceInput shared configuration template.
# Shell scripts source this file before deploying models or launching runtime.
# Use `scripts/voiceinput.sh model qwen`, `scripts/voiceinput.sh model qwen-0.6b`
# or `scripts/voiceinput.sh model funasr` to rewrite this file with a selected
# default model.
# Command-line arguments and explicit environment variables still override it.
#
# Keep one preset block uncommented below if you want a local default.

## -------------------------------------------------------------------
## FunASR preset
## -------------------------------------------------------------------
# export VOICEINPUT_ASR_MODEL="funasr"
# export VOICEINPUT_ASR_BACKEND="funasr"
# export VOICEINPUT_ASR_MODEL_ID="FunAudioLLM/Fun-ASR-Nano-2512"
# export VOICEINPUT_ASR_SOURCE_URL="https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512"
# export VOICEINPUT_ASR_MODEL_DIR="./models/FunAudioLLM/Fun-ASR-Nano-2512"
# export VOICEINPUT_ASR_REMOTE_CODE="./models/FunAudioLLM/Fun-ASR-Nano-2512/model.py"
# export VOICEINPUT_ASR_DEVICE="auto"
# export VOICEINPUT_ASR_LANGUAGE="中文"
# export VOICEINPUT_ASR_ITN="true"
# export VOICEINPUT_ASR_HOTWORDS=""

## -------------------------------------------------------------------
## Qwen preset
## -------------------------------------------------------------------
# export VOICEINPUT_ASR_MODEL="qwen"
# export VOICEINPUT_ASR_BACKEND="qwen"
# export VOICEINPUT_ASR_MODEL_ID="Qwen/Qwen3-ASR-1.7B"
# export VOICEINPUT_ASR_SOURCE_URL="https://www.modelscope.cn/collections/Qwen/Qwen3-ASR"
# export VOICEINPUT_ASR_MODEL_DIR="./models/Qwen/Qwen3-ASR-1.7B"
# export VOICEINPUT_ASR_DEVICE="auto"
# export VOICEINPUT_ASR_LANGUAGE="中文"
# export VOICEINPUT_ASR_ITN="true"
# export VOICEINPUT_ASR_HOTWORDS=""

## -------------------------------------------------------------------
## Qwen 0.6B preset
## -------------------------------------------------------------------
export VOICEINPUT_ASR_MODEL="qwen-0.6b"
export VOICEINPUT_ASR_BACKEND="qwen"
export VOICEINPUT_ASR_MODEL_ID="Qwen/Qwen3-ASR-0.6B"
export VOICEINPUT_ASR_SOURCE_URL="https://www.modelscope.cn/collections/Qwen/Qwen3-ASR"
export VOICEINPUT_ASR_MODEL_DIR="./models/Qwen/Qwen3-ASR-0.6B"
export VOICEINPUT_ASR_DEVICE="auto"
export VOICEINPUT_ASR_LANGUAGE="中文"
export VOICEINPUT_ASR_ITN="true"
export VOICEINPUT_ASR_HOTWORDS=""
EOF
      ;;
    qwen)
      cat >"$tmp_file" <<'EOF'
# VoiceInput shared configuration template.
# Shell scripts source this file before deploying models or launching runtime.
# Use `scripts/voiceinput.sh model qwen`, `scripts/voiceinput.sh model qwen-0.6b`
# or `scripts/voiceinput.sh model funasr` to rewrite this file with a selected
# default model.
# Command-line arguments and explicit environment variables still override it.
#
# Keep one preset block uncommented below if you want a local default.

## -------------------------------------------------------------------
## FunASR preset
## -------------------------------------------------------------------
# export VOICEINPUT_ASR_MODEL="funasr"
# export VOICEINPUT_ASR_BACKEND="funasr"
# export VOICEINPUT_ASR_MODEL_ID="FunAudioLLM/Fun-ASR-Nano-2512"
# export VOICEINPUT_ASR_SOURCE_URL="https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512"
# export VOICEINPUT_ASR_MODEL_DIR="./models/FunAudioLLM/Fun-ASR-Nano-2512"
# export VOICEINPUT_ASR_REMOTE_CODE="./models/FunAudioLLM/Fun-ASR-Nano-2512/model.py"
# export VOICEINPUT_ASR_DEVICE="auto"
# export VOICEINPUT_ASR_LANGUAGE="中文"
# export VOICEINPUT_ASR_ITN="true"
# export VOICEINPUT_ASR_HOTWORDS=""

## -------------------------------------------------------------------
## Qwen preset
## -------------------------------------------------------------------
export VOICEINPUT_ASR_MODEL="qwen"
export VOICEINPUT_ASR_BACKEND="qwen"
export VOICEINPUT_ASR_MODEL_ID="Qwen/Qwen3-ASR-1.7B"
export VOICEINPUT_ASR_SOURCE_URL="https://www.modelscope.cn/collections/Qwen/Qwen3-ASR"
export VOICEINPUT_ASR_MODEL_DIR="./models/Qwen/Qwen3-ASR-1.7B"
export VOICEINPUT_ASR_DEVICE="auto"
export VOICEINPUT_ASR_LANGUAGE="中文"
export VOICEINPUT_ASR_ITN="true"
export VOICEINPUT_ASR_HOTWORDS=""

## -------------------------------------------------------------------
## Qwen 0.6B preset
## -------------------------------------------------------------------
# export VOICEINPUT_ASR_MODEL="qwen-0.6b"
# export VOICEINPUT_ASR_BACKEND="qwen"
# export VOICEINPUT_ASR_MODEL_ID="Qwen/Qwen3-ASR-0.6B"
# export VOICEINPUT_ASR_SOURCE_URL="https://www.modelscope.cn/collections/Qwen/Qwen3-ASR"
# export VOICEINPUT_ASR_MODEL_DIR="./models/Qwen/Qwen3-ASR-0.6B"
# export VOICEINPUT_ASR_DEVICE="auto"
# export VOICEINPUT_ASR_LANGUAGE="中文"
# export VOICEINPUT_ASR_ITN="true"
# export VOICEINPUT_ASR_HOTWORDS=""
EOF
      ;;
  esac

  mv "$tmp_file" "$config_file"
}

voiceinput_model_impl() {
  local model=""
  local config_file="$(voiceinput_config_file_path)"

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --config-file)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --config-file 的值" >&2
          exit 2
        fi
        config_file="$2"
        shift 2
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh model <funasr|qwen|qwen-0.6b> [--config-file /path/to/voiceinput.env]

说明：
  - 这个命令会把仓库级配置文件写成你选定的默认模型
  - 之后 bootstrap/install/smoke 会默认使用这个模型，除非你再用 --model 覆盖
EOF
        exit 0
        ;;
      *)
        if [[ -z "$model" ]]; then
          model="$1"
          shift
        else
          echo "不支持的参数：$1" >&2
          exit 2
        fi
        ;;
    esac
  done

  if [[ -z "$model" ]]; then
    echo "用法：scripts/voiceinput.sh model <funasr|qwen|qwen-0.6b> [--config-file /path/to/voiceinput.env]" >&2
    exit 2
  fi

  local normalized_model
  if ! normalized_model="$(voiceinput_normalize_model_choice "$model")"; then
    echo "不支持的模型：$model" >&2
    exit 2
  fi

  mkdir -p "$(dirname "$config_file")"
  voiceinput_write_model_config "$normalized_model" "$config_file"
  echo "已写入默认模型：$normalized_model"
  echo "配置文件：$config_file"
}

voiceinput_bootstrap_impl() {
  local deploy_args=()
  local smoke_audio_file=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --backend|--model|--model-id|--source-url|--local-dir|--revision|--device|--cuda-wheel-index)
        if [[ $# -lt 2 ]]; then
          echo "缺少 $1 的值" >&2
          exit 2
        fi
        if [[ "$1" == "--model" ]]; then
          local normalized_model
          if ! normalized_model="$(voiceinput_normalize_model_choice "$2")"; then
            echo "不支持的模型：$2" >&2
            exit 2
          fi
          case "$normalized_model" in
            qwen-0.6b)
              deploy_args+=(
                "--backend" "$(voiceinput_model_backend_for_choice "$normalized_model")"
                "--model-id" "$(voiceinput_model_id_for_choice "$normalized_model")"
                "--source-url" "$(voiceinput_model_source_url_for_choice "$normalized_model")"
                "--local-dir" "$(voiceinput_model_local_dir_for_choice "$normalized_model")"
              )
              ;;
            *)
              deploy_args+=("--backend" "$2")
              ;;
          esac
        else
          deploy_args+=("$1" "$2")
        fi
        shift 2
        ;;
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
  scripts/voiceinput.sh bootstrap [部署参数...] [--audio-file /path/to/audio.wav]

说明：
  - 未传 --audio-file 时，只执行 Python 环境和模型部署
  - 传入 --audio-file 时，会在部署完成后自动运行 macOS smoke
  - 默认会读取 config/voiceinput.env；如果要换文件，可以设置 VOICEINPUT_CONFIG_FILE
  - 部署参数会原样传给 deploy_funasr_model.py

常用部署参数：
  --model funasr|qwen|qwen-0.6b
  --backend funasr|qwen
  --model-id
  --source-url
  --local-dir
  --revision
  --skip-existing
  --install-cuda
  --device auto|cpu|cuda|mps
  --cuda-wheel-index

说明：
  - `--model qwen-0.6b` 会写入 Qwen3-ASR-0.6B 的模型 ID、来源和目录
EOF
        exit 0
        ;;
      *)
        deploy_args+=("$1")
        shift
        ;;
    esac
  done

  voiceinput_configure_rust_mirror
  voiceinput_clear_proxy_env
  export UV_DEFAULT_INDEX="${UV_DEFAULT_INDEX:-https://mirrors.aliyun.com/pypi/simple/}"

  voiceinput_ensure_cargo
  voiceinput_ensure_rustfmt
  voiceinput_ensure_uv

  cd "$REPO_ROOT"

  echo "正在创建或更新 Python 虚拟环境：.venv"
  UV_VENV_CLEAR=1 uv venv .venv --python "$(command -v python3.12)"

  echo "正在安装模型下载依赖"
  uv pip install -r scripts/requirements-asr-base.txt

  echo "正在安装 ASR 运行时依赖"
  uv pip install -r scripts/requirements-asr-runtime.txt

  echo "正在部署本地 ASR 模型"
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
}

voiceinput_macos_smoke_impl() {
  local audio_file=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --audio-file)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --audio-file 的值" >&2
          exit 2
        fi
        audio_file="$2"
        shift 2
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh macos smoke --audio-file /path/to/audio.wav

说明：
  - 默认会读取 config/voiceinput.env；如果要换文件，可以设置 VOICEINPUT_CONFIG_FILE
EOF
        exit 0
        ;;
      *)
        echo "不支持的参数：$1" >&2
        exit 2
        ;;
    esac
  done

  if [[ -z "$audio_file" ]]; then
    echo "用法：scripts/voiceinput.sh macos smoke --audio-file /path/to/audio.wav" >&2
    exit 2
  fi

  voiceinput_ensure_uv
  cd "$REPO_ROOT"
  uv run -- cargo run -p voice-input-macos -- --audio-file "$audio_file"
}

voiceinput_linux_smoke_impl() {
  local audio_file=""
  local backend="ibus"
  local smoke_args=()

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --audio-file)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --audio-file 的值" >&2
          exit 2
        fi
        audio_file="$2"
        smoke_args+=("$1" "$2")
        shift 2
        ;;
      --model)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --model 的值" >&2
          exit 2
        fi
        if ! voiceinput_apply_model_choice_env "$2"; then
          echo "不支持的模型：$2" >&2
          exit 2
        fi
        shift 2
        ;;
      --backend)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --backend 的值" >&2
          exit 2
        fi
        backend="$2"
        smoke_args+=("$1" "$2")
        shift 2
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh linux smoke --audio-file /path/to/audio.wav [--model funasr|qwen|qwen-0.6b] [--backend ibus|fcitx5]

说明：
  - 默认使用 IBus
  - 需要先准备好 Python ASR 环境和本地模型
  - 默认会读取 config/voiceinput.env；如果要换文件，可以设置 VOICEINPUT_CONFIG_FILE
  - `--model` 会通过环境变量传给运行时，`qwen-0.6b` 也可用
EOF
        exit 0
        ;;
      *)
        smoke_args+=("$1")
        shift
        ;;
    esac
  done

  if [[ -z "$audio_file" ]]; then
    echo "用法：scripts/voiceinput.sh linux smoke --audio-file /path/to/audio.wav [--model funasr|qwen|qwen-0.6b] [--backend ibus|fcitx5]" >&2
    exit 2
  fi

  voiceinput_ensure_uv
  voiceinput_ensure_linux_dev_deps
  cd "$REPO_ROOT"
  uv run -- cargo run -p voice-input-linux --features ibus -- "${smoke_args[@]}"
}

voiceinput_macos_install_impl() {
  local audio_file=""
  local run_smoke_after_install=false
  local bootstrap_args=()

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --backend|--model|--model-id|--source-url|--local-dir|--revision|--device|--cuda-wheel-index|--install-cuda|--skip-existing)
        if [[ "$1" == "--model" ]]; then
          if [[ $# -lt 2 ]]; then
            echo "缺少 $1 的值" >&2
            exit 2
          fi
          local normalized_model
          if ! normalized_model="$(voiceinput_normalize_model_choice "$2")"; then
            echo "不支持的模型：$2" >&2
            exit 2
          fi
          case "$normalized_model" in
            qwen-0.6b)
              bootstrap_args+=(
                "--backend" "$(voiceinput_model_backend_for_choice "$normalized_model")"
                "--model-id" "$(voiceinput_model_id_for_choice "$normalized_model")"
                "--source-url" "$(voiceinput_model_source_url_for_choice "$normalized_model")"
                "--local-dir" "$(voiceinput_model_local_dir_for_choice "$normalized_model")"
              )
              ;;
            *)
              bootstrap_args+=("--backend" "$2")
              ;;
          esac
          shift 2
          continue
        fi
        bootstrap_args+=("$1")
        if [[ "$1" != "--install-cuda" && "$1" != "--skip-existing" ]]; then
          if [[ $# -lt 2 ]]; then
            echo "缺少 $1 的值" >&2
            exit 2
          fi
          bootstrap_args+=("$2")
          shift 2
        else
          shift
        fi
        ;;
      --audio-file)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --audio-file 的值" >&2
          exit 2
        fi
        audio_file="$2"
        run_smoke_after_install=true
        shift 2
        ;;
      --skip-smoke)
        run_smoke_after_install=false
        audio_file=""
        shift
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh macos install [ASR 部署参数...] [--audio-file /path/to/audio.wav]

说明：
  - 先创建 Python 环境并下载本地模型
  - 再打包系统级 macOS 输入法应用
  - 最后复制到 ~/Library/Input Methods/
  - 安装完成后会自动执行启用脚本
  - 如果传入 --audio-file，会在安装后自动运行一次 smoke 验证
  - 默认会读取 config/voiceinput.env；如果要换文件，可以设置 VOICEINPUT_CONFIG_FILE
  - 可选 ASR 部署参数会原样传给 scripts/voiceinput.sh bootstrap
  - `--model funasr|qwen|qwen-0.6b` 会分别选择 FunASR、Qwen 1.7B、Qwen 0.6B
EOF
        exit 0
        ;;
      *)
        bootstrap_args+=("$1")
        shift
        ;;
    esac
  done

  cd "$REPO_ROOT"
  voiceinput_bootstrap_impl "${bootstrap_args[@]}"

  echo "正在打包系统级输入法应用"
  voiceinput_package_macos_impl

  # shellcheck disable=SC1090
  source scripts/macos_input_method_common.sh
  local install_dir="$HOME/Library/Input Methods"
  local app_bundle="dist/VoiceInput.app"
  local target_bundle="$install_dir/VoiceInput.app"

  echo "正在安装到系统输入法目录：$target_bundle"
  target_bundle="$(voiceinput_sync_bundle "$app_bundle" "$install_dir")"

  echo "安装完成"
  echo "已安装到：$target_bundle"
  echo "启用完成"
  echo "请重新登录或重启输入法服务，然后在系统输入法列表中启用 VoiceInput"
  echo "首次运行前建议授予“麦克风”和“辅助功能”权限"

  if [[ "$run_smoke_after_install" == true ]]; then
    echo "正在运行 smoke 验证"
    voiceinput_macos_smoke_impl --audio-file "$audio_file"
  fi
}

voiceinput_linux_install_impl() {
  local backend="ibus"
  local audio_file=""
  local run_smoke_after_bootstrap=false
  local run_live_app_after_bootstrap=true
  local deploy_args=()

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
      --model)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --model 的值" >&2
          exit 2
        fi
        local normalized_model
        if ! normalized_model="$(voiceinput_normalize_model_choice "$2")"; then
          echo "不支持的模型：$2" >&2
          exit 2
        fi
        case "$normalized_model" in
          qwen-0.6b)
            deploy_args+=(
              "--backend" "$(voiceinput_model_backend_for_choice "$normalized_model")"
              "--model-id" "$(voiceinput_model_id_for_choice "$normalized_model")"
              "--source-url" "$(voiceinput_model_source_url_for_choice "$normalized_model")"
              "--local-dir" "$(voiceinput_model_local_dir_for_choice "$normalized_model")"
            )
            ;;
          *)
            deploy_args+=("$1" "$2")
            ;;
        esac
        shift 2
        ;;
      --no-launch)
        run_live_app_after_bootstrap=false
        shift
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh linux install [--backend ibus|fcitx5] [--model funasr|qwen|qwen-0.6b] [--audio-file /path/to/audio.wav]

说明：
  - 默认先执行 Linux bootstrap，准备 Python 环境并下载模型
  - 会自动安装 Ubuntu 20.04 常用的 Linux 编译依赖，如 pkg-config、libdbus-1-dev、libibus-1.0-dev
  - 然后自动启动 Linux 常驻托盘版
  - 如果传入 --audio-file，会在准备完成后自动跑一次 Linux smoke
  - 默认会读取 config/voiceinput.env；如果要换文件，可以设置 VOICEINPUT_CONFIG_FILE
  - --backend 只影响 Linux 常驻版 / smoke 的宿主后端
  - --model 会原样传给 scripts/voiceinput.sh bootstrap，用来选择 ASR 模型
  - `--model qwen-0.6b` 会切到 Qwen3-ASR-0.6B
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
  voiceinput_ensure_linux_dev_deps
  voiceinput_bootstrap_impl "${deploy_args[@]}"

  if [[ "$run_smoke_after_bootstrap" == true ]]; then
    echo "正在运行 Linux smoke"
    voiceinput_linux_smoke_impl --audio-file "$audio_file" --backend "$backend"
    exit 0
  fi

  if [[ "$run_live_app_after_bootstrap" == true ]]; then
    echo "正在启动 Linux 常驻托盘版"
    voiceinput_ensure_uv
    voiceinput_refresh_cargo_path
    local cargo_bin
    cargo_bin="$(command -v cargo || true)"
    if [[ -z "$cargo_bin" && -x "${HOME}/.cargo/bin/cargo" ]]; then
      cargo_bin="${HOME}/.cargo/bin/cargo"
    fi
    uv run -- "$cargo_bin" run -p voice-input-linux --features ibus --bin voice-input-linux-app -- --backend "$backend"
  fi
}

voiceinput_package_macos_impl() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --help|-h)
        cat <<'EOF'
用法：
  scripts/voiceinput.sh macos package

说明：
  - 这个命令只负责打包 macOS 输入法
  - 不会安装到系统目录，也不会启动 smoke
EOF
        exit 0
        ;;
      *)
        echo "不支持的参数：$1" >&2
        exit 2
        ;;
    esac
  done

  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "这个脚本只能在 macOS 上运行。" >&2
    exit 1
  fi

  cd "$REPO_ROOT"
  voiceinput_ensure_cargo
  voiceinput_ensure_rustfmt

  local app_name="${APP_NAME:-VoiceInput}"
  local container_bundle_id="${CONTAINER_BUNDLE_ID:-com.example.voiceinput.container}"
  local extension_name="${EXTENSION_NAME:-VoiceInput.appex}"
  local dist_dir="${DIST_DIR:-dist}"
  local app_bundle="$dist_dir/$app_name.app"
  local contents_dir="$app_bundle/Contents"
  local macos_dir="$contents_dir/MacOS"
  local resources_dir="$contents_dir/Resources"
  local plugins_dir="$contents_dir/PlugIns"
  local extension_bundle="$plugins_dir/$extension_name"
  local extension_contents_dir="$extension_bundle/Contents"
  local extension_macos_dir="$extension_contents_dir/MacOS"
  local extension_resources_dir="$extension_contents_dir/Resources"
  local plist_file="$contents_dir/Info.plist"
  local app_bin_name="voice-input-macos-app"
  local ime_bin_name="voice-input-macos-ime"
  local app_bin_path="target/release/$app_bin_name"
  local ime_bin_path="target/release/$ime_bin_name"
  local icon_source="/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericApplicationIcon.icns"
  local icon_name="VoiceInput.icns"

  echo "正在编译 macOS 容器和系统级 IME 入口"
  cargo build -p voice-input-macos --bin "$app_bin_name" --bin "$ime_bin_name" --release

  echo "正在组装应用包：$app_bundle"
  rm -rf "$app_bundle"
  mkdir -p "$macos_dir" "$resources_dir" "$plugins_dir" "$extension_macos_dir" "$extension_resources_dir"
  cp "$app_bin_path" "$macos_dir/$app_name"
  cp "$ime_bin_path" "$extension_macos_dir/VoiceInputInputMethod"
  cp "$icon_source" "$resources_dir/$icon_name"
  cp "$icon_source" "$extension_resources_dir/$icon_name"

  cat >"$contents_dir/PkgInfo" <<'EOF'
APPL????
EOF

  cat >"$contents_dir/version.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
</dict>
</plist>
EOF

  cat >"$plist_file" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>zh_CN</string>
  <key>CFBundleDisplayName</key>
  <string>$app_name</string>
  <key>CFBundleExecutable</key>
  <string>$app_name</string>
  <key>CFBundleIdentifier</key>
  <string>$container_bundle_id</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$app_name</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleSupportedPlatforms</key>
  <array>
    <string>MacOSX</string>
  </array>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>CFBundleSignature</key>
  <string>????</string>
  <key>LSUIElement</key>
  <true/>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
  <key>LSBackgroundOnly</key>
  <true/>
  <key>CFBundleIconFile</key>
  <string>$icon_name</string>
</dict>
</plist>
EOF

  cat >"$extension_contents_dir/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>zh_CN</string>
  <key>CFBundleDisplayName</key>
  <string>VoiceInput</string>
  <key>CFBundleExecutable</key>
  <string>VoiceInputInputMethod</string>
  <key>CFBundleIdentifier</key>
  <string>com.example.voiceinput.inputmethod</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>VoiceInput</string>
  <key>CFBundlePackageType</key>
  <string>XPC!</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>NSExtension</key>
  <dict>
    <key>NSExtensionAttributes</key>
    <dict>
      <key>InputMethodConnectionName</key>
      <string>com.example.voiceinput.inputmethod_Connection</string>
      <key>InputMethodServerControllerClass</key>
      <string>VoiceInputInputController</string>
      <key>TISInputSourceID</key>
      <string>com.example.voiceinput.inputmethod.default</string>
      <key>TISIntendedLanguage</key>
      <string>zh-Hans</string>
      <key>TISParticipatesInTouchBar</key>
      <true/>
      <key>tsInputMethodCharacterRepertoireKey</key>
      <array>
        <string>Latn</string>
      </array>
      <key>NSMainNibFile</key>
      <string>KeyboardService</string>
      <key>CFBundleHelpBookFolder</key>
      <string>VoiceInputHelp</string>
      <key>CFBundleHelpBookName</key>
      <string>VoiceInput Help</string>
    </dict>
    <key>NSExtensionPointIdentifier</key>
    <string>com.apple.textinputmethod-services</string>
    <key>NSExtensionPrincipalClass</key>
    <string>IMKExtension</string>
  </dict>
  <key>CFBundleIconFile</key>
  <string>$icon_name</string>
</dict>
</plist>
EOF

  echo "正在输出应用包：$app_bundle"
  echo "Container Bundle ID: $container_bundle_id"
  echo "Input Method Bundle ID: com.example.voiceinput.inputmethod"
}

voiceinput_reinstall_macos_impl() {
  local app_bundle="${APP_BUNDLE:-dist/VoiceInput.app}"
  local install_dir="${INSTALL_DIR:-$HOME/Library/Input Methods}"

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --app-bundle)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --app-bundle 的值" >&2
          exit 2
        fi
        app_bundle="$2"
        shift 2
        ;;
      --help|-h)
        cat <<'EOF'
用法：
  scripts/voiceinput.sh macos reinstall [--app-bundle /path/to/VoiceInput.app]
EOF
        exit 0
        ;;
      *)
        echo "不支持的参数：$1" >&2
        exit 2
        ;;
    esac
  done

  if [[ ! -d "$app_bundle" ]]; then
    echo "找不到应用包：$app_bundle" >&2
    echo "请先运行 scripts/voiceinput.sh macos package" >&2
    exit 1
  fi

  cd "$REPO_ROOT"
  # shellcheck disable=SC1090
  source scripts/macos_input_method_common.sh

  echo "正在刷新系统输入法安装：$install_dir/VoiceInput.app"
  voiceinput_sync_bundle "$app_bundle" "$install_dir"
  echo "刷新完成"
  echo "启用完成"
}

voiceinput_enable_macos_impl() {
  local app_bundle="${APP_BUNDLE:-$HOME/Library/Input Methods/VoiceInput.app}"
  local bundle_id="${BUNDLE_ID:-com.example.voiceinput.inputmethod}"
  local input_mode_id="${INPUT_MODE_ID:-com.example.voiceinput.inputmethod.default}"
  local input_method_kind="${INPUT_METHOD_KIND:-Input Mode}"
  local extension_bundle="${EXTENSION_BUNDLE:-$app_bundle/Contents/PlugIns/VoiceInput.appex}"

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --app-bundle)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --app-bundle 的值" >&2
          exit 2
        fi
        app_bundle="$2"
        extension_bundle="$app_bundle/Contents/PlugIns/VoiceInput.appex"
        shift 2
        ;;
      --bundle-id)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --bundle-id 的值" >&2
          exit 2
        fi
        bundle_id="$2"
        shift 2
        ;;
      --input-mode-id)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --input-mode-id 的值" >&2
          exit 2
        fi
        input_mode_id="$2"
        shift 2
        ;;
      --help|-h)
        cat <<'EOF'
用法：
  scripts/voiceinput.sh macos enable [--app-bundle /path/to/VoiceInput.app] [--bundle-id ...] [--input-mode-id ...]
EOF
        exit 0
        ;;
      *)
        echo "不支持的参数：$1" >&2
        exit 2
        ;;
    esac
  done

  if [[ ! -d "$app_bundle" ]]; then
    echo "找不到应用包：$app_bundle" >&2
    echo "请先运行 scripts/voiceinput.sh macos reinstall 或 scripts/voiceinput.sh macos dev-install" >&2
    exit 1
  fi

  if [[ ! -d "$extension_bundle" ]]; then
    echo "找不到扩展包：$extension_bundle" >&2
    echo "请确认这个 app bundle 内含有 VoiceInput.appex" >&2
    exit 1
  fi

  cd "$REPO_ROOT"
  # shellcheck disable=SC1090
  source scripts/macos_input_method_common.sh
  voiceinput_enable_bundle "$app_bundle" "$bundle_id" "$input_mode_id" "$input_method_kind" "$extension_bundle"
  echo "启用完成"
}

voiceinput_dev_install_macos_impl() {
  local app_bundle="${APP_BUNDLE:-dist/VoiceInput.app}"

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --app-bundle)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --app-bundle 的值" >&2
          exit 2
        fi
        app_bundle="$2"
        shift 2
        ;;
      --help|-h)
        cat <<'EOF'
用法：
  scripts/voiceinput.sh macos dev-install [--app-bundle /path/to/VoiceInput.app]
EOF
        exit 0
        ;;
      *)
        echo "不支持的参数：$1" >&2
        exit 2
        ;;
    esac
  done

  cd "$REPO_ROOT"
  voiceinput_package_macos_impl
  # shellcheck disable=SC1090
  source scripts/macos_input_method_common.sh
  voiceinput_sync_bundle "$app_bundle" "$HOME/Library/Input Methods"
  echo "开发安装完成"
}

voiceinput_dump_macos_state_impl() {
  local app_bundle="${APP_BUNDLE:-$HOME/Library/Input Methods/VoiceInput.app}"
  local extension_bundle="${EXTENSION_BUNDLE:-$app_bundle/Contents/PlugIns/VoiceInput.appex}"
  local target_bundle_id="${TARGET_BUNDLE_ID:-com.example.voiceinput.inputmethod}"
  local lsregister="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"

  cd "$REPO_ROOT"

  echo "== App Bundle =="
  echo "APP_BUNDLE=$app_bundle"
  if [[ -d "$app_bundle" ]]; then
    plutil -p "$app_bundle/Contents/Info.plist" 2>/dev/null || true
    echo
    echo "xattr:"
    xattr -l "$app_bundle" 2>/dev/null || echo "(none)"
  else
    echo "bundle not found"
  fi

  echo
  echo "== Extension Bundle =="
  echo "EXTENSION_BUNDLE=$extension_bundle"
  if [[ -d "$extension_bundle" ]]; then
    plutil -p "$extension_bundle/Contents/Info.plist" 2>/dev/null || true
    echo
    echo "xattr:"
    xattr -l "$extension_bundle" 2>/dev/null || echo "(none)"
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
    TARGET_BUNDLE_ID="$target_bundle_id" swift - <<'SWIFT'
import Carbon.HIToolbox
import Foundation

let target = ProcessInfo.processInfo.environment["TARGET_BUNDLE_ID"] ?? ""
func dumpSources(includeAllInstalled: Bool, label: String) -> Bool {
  guard let rawList = TISCreateInputSourceList(nil, includeAllInstalled) else {
    print("\(label): <no sources>")
    return false
  }

  let list = rawList.takeRetainedValue() as NSArray
  var found = false

  print("\(label): count=\(list.count)")
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

    if let sourceID = TISGetInputSourceProperty(src, kTISPropertyInputSourceID) {
      let inputSourceID = unsafeBitCast(sourceID, to: CFString.self) as String
      fields.append("sourceID=\(inputSourceID)")
      if inputSourceID == target {
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

  return found
}

var found = dumpSources(includeAllInstalled: false, label: "enabled")
if !found {
  found = dumpSources(includeAllInstalled: true, label: "installed")
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
  if [[ -x "$lsregister" ]]; then
    "$lsregister" -dump 2>/dev/null | rg -n "VoiceInput|${target_bundle_id//./\\.}|InputMethodConnectionName|InputMethodServerControllerClass|tsInputMethodCharacterRepertoireKey" || true
  else
    echo "lsregister not found"
  fi

  echo
  echo "== mdls =="
  if [[ -d "$app_bundle" ]]; then
    mdls -name kMDItemCFBundleIdentifier -name kMDItemKind -name kMDItemContentType "$app_bundle" 2>/dev/null || true
  fi
}

voiceinput_linux_dev_streaming_impl() {
  local run_prepare=false
  local restart_server=false
  local stop_server=false
  local app_args=()

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --prepare)
        run_prepare=true
        shift
        ;;
      --restart-server)
        restart_server=true
        shift
        ;;
      --stop-server)
        stop_server=true
        shift
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh linux dev-streaming [--prepare] [--restart-server] [--stop-server] [-- 传给 Linux 常驻应用的参数...]
EOF
        exit 0
        ;;
      --)
        shift
        app_args+=("$@")
        break
        ;;
      *)
        app_args+=("$1")
        shift
        ;;
    esac
  done

  has_app_arg() {
    local needle="$1"
    local arg
    for arg in "${app_args[@]}"; do
      if [[ "$arg" == "$needle" ]]; then
        return 0
      fi
    done
    return 1
  }

  if ! has_app_arg "--double-ctrl-window-ms"; then
    app_args=("--double-ctrl-window-ms" "300" "${app_args[@]}")
  fi

  local socket_path="${VOICEINPUT_FUNASR_SOCKET_PATH:-/tmp/voiceinput-funasr.sock}"
  local server_pid_file="${VOICEINPUT_FUNASR_PID_FILE:-/tmp/voiceinput-funasr.pid}"
  local server_log="${VOICEINPUT_FUNASR_LOG_FILE:-/tmp/voiceinput-funasr.log}"

  socket_is_alive() {
    local target_socket="$1"
    python3 - "$target_socket" <<'PY'
import socket
import sys

path = sys.argv[1]
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.settimeout(0.2)
try:
    sock.connect(path)
except OSError:
    raise SystemExit(1)
finally:
    sock.close()
raise SystemExit(0)
PY
  }

  cd "$REPO_ROOT"
  voiceinput_ensure_uv
  voiceinput_refresh_cargo_path

  if [[ "$run_prepare" == true ]]; then
    voiceinput_bootstrap_impl --skip-existing
  fi

  if [[ ! -f ".venv/bin/python" ]]; then
    echo "未找到 .venv。请先运行 scripts/voiceinput.sh bootstrap 或 scripts/voiceinput.sh linux dev-streaming --prepare" >&2
    exit 2
  fi

  local server_running=false
  if [[ -S "$socket_path" ]] && socket_is_alive "$socket_path"; then
    server_running=true
  elif [[ -S "$socket_path" ]]; then
    rm -f "$socket_path"
  fi

  if [[ "$stop_server" == true ]]; then
    if [[ -f "$server_pid_file" ]]; then
      local server_pid
      server_pid="$(cat "$server_pid_file" 2>/dev/null || true)"
      if [[ -n "${server_pid:-}" ]] && kill -0 "$server_pid" >/dev/null 2>&1; then
        echo "正在停止 FunASR 开发服务：$server_pid"
        kill "$server_pid" >/dev/null 2>&1 || true
      fi
    fi
    rm -f "$server_pid_file" "$socket_path"
    echo "FunASR 开发服务已停止"
    exit 0
  fi

  if [[ "$restart_server" == true ]]; then
    if [[ -f "$server_pid_file" ]]; then
      local server_pid
      server_pid="$(cat "$server_pid_file" 2>/dev/null || true)"
      if [[ -n "${server_pid:-}" ]] && kill -0 "$server_pid" >/dev/null 2>&1; then
        echo "正在重启 FunASR 开发服务：$server_pid"
        kill "$server_pid" >/dev/null 2>&1 || true
      fi
    fi
    rm -f "$server_pid_file" "$socket_path"
    server_running=false
  fi

  if [[ "$server_running" == false ]]; then
    echo "正在启动常驻 FunASR 开发服务"
    nohup uv run -- python scripts/funasr_stream_server.py \
      --socket-path "$socket_path" \
      --model-dir "./models/FunAudioLLM/Fun-ASR-Nano-2512" \
      >"$server_log" 2>&1 &
    local server_pid
    server_pid=$!
    echo "$server_pid" >"$server_pid_file"

    for _ in $(seq 1 300); do
      if [[ -S "$socket_path" ]]; then
        break
      fi
      if ! kill -0 "$server_pid" >/dev/null 2>&1; then
        echo "FunASR 开发服务启动失败，日志如下：" >&2
        cat "$server_log" >&2 || true
        rm -f "$server_pid_file"
        exit 1
      fi
      sleep 1
    done

    if [[ ! -S "$socket_path" ]]; then
      echo "等待 FunASR 开发服务就绪超时，日志如下：" >&2
      cat "$server_log" >&2 || true
      rm -f "$server_pid_file"
      exit 1
    fi
  else
    echo "复用已有 FunASR 开发服务：$socket_path"
  fi

  echo "FunASR 开发服务已就绪：$socket_path"
  VOICEINPUT_FUNASR_SOCKET="$socket_path" \
    uv run -- cargo run -p voice-input-linux --features ibus --bin voice-input-linux-app -- "${app_args[@]}"
}

usage() {
  cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh <command> [args...]

主命令：
  bootstrap              准备 Python 环境、安装依赖并下载模型
  model                  写入仓库级默认模型配置

平台子命令：
  macos install          打包并安装 macOS 输入法
  macos smoke            运行 macOS smoke
  macos package          只打包 macOS 输入法
  macos reinstall        刷新 macOS 输入法安装
  macos enable           启用已安装的 macOS 输入法
  macos dev-install      开发时打包 + 刷新 + 启用
  macos dump-state       导出 macOS 输入法状态
  linux install          安装并启动 Linux 常驻版
  linux smoke            运行 Linux smoke
  linux dev              启动 Linux 开发常驻服务
  linux dev-streaming    启动 Linux FunASR 流式开发服务

说明：
  - 所有子命令都会继续兼容现有脚本参数
  - 默认配置来自 config/voiceinput.env
  - 推荐优先使用 `scripts/voiceinput.sh <command> ...`
  - 旧的扁平命令名（例如 macos-install、linux-smoke）仍然可用
EOF
}

cmd="${1:-}"
if [[ -z "$cmd" || "$cmd" == "--help" || "$cmd" == "-h" ]]; then
  usage
  exit 0
fi

shift || true

if [[ "$cmd" == "macos" || "$cmd" == "linux" ]]; then
  platform="$cmd"
  action="${1:-}"
  if [[ -z "$action" || "$action" == "--help" || "$action" == "-h" ]]; then
    usage
    exit 0
  fi
  shift || true
  cmd="${platform}-${action}"
fi

case "$cmd" in
  model)
    voiceinput_model_impl "$@"
    ;;
  bootstrap)
    voiceinput_bootstrap_impl "$@"
    ;;
  macos-install)
    voiceinput_macos_install_impl "$@"
    ;;
  macos-smoke)
    voiceinput_macos_smoke_impl "$@"
    ;;
  linux-install)
    voiceinput_linux_install_impl "$@"
    ;;
  linux-smoke)
    voiceinput_linux_smoke_impl "$@"
    ;;
  macos-package)
    voiceinput_package_macos_impl "$@"
    ;;
  macos-reinstall)
    voiceinput_reinstall_macos_impl "$@"
    ;;
  macos-enable)
    voiceinput_enable_macos_impl "$@"
    ;;
  macos-dev-install)
    voiceinput_dev_install_macos_impl "$@"
    ;;
  macos-dump-state)
    voiceinput_dump_macos_state_impl "$@"
    ;;
  linux-dev)
    voiceinput_linux_dev_streaming_impl "$@"
    ;;
  linux-dev-streaming)
    voiceinput_linux_dev_streaming_impl "$@"
    ;;
  *)
    echo "不支持的命令：$cmd" >&2
    usage
    exit 2
    ;;
esac
