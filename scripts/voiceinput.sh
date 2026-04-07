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

voiceinput_find_cargo_bin() {
  local cargo_bin
  cargo_bin="$(command -v cargo || true)"
  if [[ -z "$cargo_bin" && -x "${HOME}/.cargo/bin/cargo" ]]; then
    cargo_bin="${HOME}/.cargo/bin/cargo"
  fi
  printf '%s\n' "$cargo_bin"
}

voiceinput_run_cli() {
  local cargo_bin
  cargo_bin="$(voiceinput_find_cargo_bin)"
  if [[ -z "$cargo_bin" ]]; then
    echo "未找到 cargo，可先执行 scripts/voiceinput.sh bootstrap" >&2
    exit 1
  fi

  uv run -- "$cargo_bin" run -p voice-input-cli -- "$@"
}

voiceinput_run_cli_release() {
  local cargo_bin
  cargo_bin="$(voiceinput_find_cargo_bin)"
  if [[ -z "$cargo_bin" ]]; then
    echo "未找到 cargo，可先执行 scripts/voiceinput.sh bootstrap" >&2
    exit 1
  fi

  uv run -- "$cargo_bin" run -p voice-input-cli --release -- "$@"
}

voiceinput_run_cli_linux() {
  local cargo_bin
  cargo_bin="$(voiceinput_find_cargo_bin)"
  if [[ -z "$cargo_bin" ]]; then
    echo "未找到 cargo，可先执行 scripts/voiceinput.sh bootstrap" >&2
    exit 1
  fi

  uv run -- "$cargo_bin" run -p voice-input-cli --features linux-ibus-smoke -- "$@"
}

voiceinput_run_bootstrap_args() {
  if (($# > 0)); then
    voiceinput_bootstrap_impl "$@"
  else
    voiceinput_bootstrap_impl
  fi
}

voiceinput_run_platform_smoke() {
  local platform="$1"
  local audio_file="$2"
  local backend="${3:-ibus}"

  case "$platform" in
    macos)
      echo "正在运行 macOS smoke 验证"
      voiceinput_macos_smoke_impl --audio-file "$audio_file"
      ;;
    linux)
      echo "正在运行 Linux smoke"
      voiceinput_linux_smoke_impl --audio-file "$audio_file" --backend "$backend"
      ;;
    windows)
      echo "正在运行 Windows smoke 验证"
      voiceinput_windows_smoke_impl --audio-file "$audio_file"
      ;;
    *)
      echo "不支持的 smoke 平台：$platform" >&2
      exit 2
      ;;
  esac
}

voiceinput_run_platform_live() {
  local platform="$1"
  local backend="${2:-ibus}"

  voiceinput_ensure_cargo
  voiceinput_ensure_uv

  case "$platform" in
    macos)
      echo "正在启动 macOS 常驻应用"
      voiceinput_run_cli_release live macos
      ;;
    linux)
      echo "正在启动 Linux 常驻托盘版"
      voiceinput_refresh_cargo_path
      voiceinput_run_cli_linux live linux --backend "$backend"
      ;;
    windows)
      echo "正在启动 Windows 常驻应用"
      voiceinput_run_cli_release live windows
      ;;
    *)
      echo "不支持的 live 平台：$platform" >&2
      exit 2
      ;;
  esac
}

voiceinput_ensure_linux_dev_deps() {
  local -a required_packages=(
    pkg-config
    libdbus-1-dev
    libibus-1.0-dev
    libx11-dev
    libasound2-dev
    portaudio19-dev
  )
  local -a missing_packages=()

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
  python3 "$SCRIPT_DIR/model_catalog.py" normalize "${1:-}"
}

voiceinput_model_field_for_choice() {
  local choice="$1"
  local field="$2"
  python3 "$SCRIPT_DIR/model_catalog.py" get "$choice" "$field"
}

voiceinput_model_backend_for_choice() {
  voiceinput_model_field_for_choice "${1:-}" "backend"
}

voiceinput_model_id_for_choice() {
  voiceinput_model_field_for_choice "${1:-}" "model_id"
}

voiceinput_model_source_url_for_choice() {
  voiceinput_model_field_for_choice "${1:-}" "source_url"
}

voiceinput_model_local_dir_for_choice() {
  voiceinput_model_field_for_choice "${1:-}" "model_dir"
}

voiceinput_model_remote_code_for_choice() {
  voiceinput_model_field_for_choice "${1:-}" "remote_code"
}

voiceinput_apply_model_choice_env() {
  local choice
  choice="$(voiceinput_normalize_model_choice "${1:-}")" || return 1

  export VOICEINPUT_ASR_MODEL="$choice"
  export VOICEINPUT_ASR_BACKEND="$(voiceinput_model_backend_for_choice "$choice")"
  export VOICEINPUT_ASR_MODEL_ID="$(voiceinput_model_id_for_choice "$choice")"
  export VOICEINPUT_ASR_SOURCE_URL="$(voiceinput_model_source_url_for_choice "$choice")"
  export VOICEINPUT_ASR_MODEL_DIR="$(voiceinput_model_local_dir_for_choice "$choice")"
  local remote_code
  remote_code="$(voiceinput_model_remote_code_for_choice "$choice")"
  if [[ -n "$remote_code" ]]; then
    export VOICEINPUT_ASR_REMOTE_CODE="$remote_code"
  else
    unset VOICEINPUT_ASR_REMOTE_CODE
  fi
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

  python3 "$SCRIPT_DIR/model_catalog.py" render-config-file "$normalized_model" >"$tmp_file"

  mv "$tmp_file" "$config_file"
}

VOICEINPUT_EXPANDED_MODEL_ARGS=()

voiceinput_expand_model_args() {
  local mode="$1"
  local model="$2"
  local normalized_model
  if ! normalized_model="$(voiceinput_normalize_model_choice "$model")"; then
    return 1
  fi

  VOICEINPUT_EXPANDED_MODEL_ARGS=()
  case "$normalized_model" in
    qwen-0.6b)
      VOICEINPUT_EXPANDED_MODEL_ARGS=(
        "--backend" "$(voiceinput_model_backend_for_choice "$normalized_model")"
        "--model-id" "$(voiceinput_model_id_for_choice "$normalized_model")"
        "--source-url" "$(voiceinput_model_source_url_for_choice "$normalized_model")"
        "--local-dir" "$(voiceinput_model_local_dir_for_choice "$normalized_model")"
      )
      ;;
    *)
      case "$mode" in
        backend)
          VOICEINPUT_EXPANDED_MODEL_ARGS=("--backend" "$normalized_model")
          ;;
        passthrough)
          VOICEINPUT_EXPANDED_MODEL_ARGS=("--model" "$normalized_model")
          ;;
        *)
          echo "不支持的模型展开模式：$mode" >&2
          return 2
          ;;
      esac
      ;;
  esac
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
  local -a deploy_args=()
  local smoke_audio_file=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --backend|--model|--model-id|--source-url|--local-dir|--revision|--device|--cuda-wheel-index)
        if [[ $# -lt 2 ]]; then
          echo "缺少 $1 的值" >&2
          exit 2
        fi
        if [[ "$1" == "--model" ]]; then
          if ! voiceinput_expand_model_args backend "$2"; then
            echo "不支持的模型：$2" >&2
            exit 2
          fi
          deploy_args+=("${VOICEINPUT_EXPANDED_MODEL_ARGS[@]}")
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

  if [[ -d ".venv" ]]; then
    echo "正在复用 Python 虚拟环境：.venv"
  else
    echo "正在创建 Python 虚拟环境：.venv"
    uv venv .venv --python "$(command -v python3.12)"
  fi

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
    uv run -- cargo run -p voice-input-macos --bin voice-input-macos -- --audio-file "$smoke_audio_file"
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

  voiceinput_ensure_cargo
  voiceinput_ensure_uv
  cd "$REPO_ROOT"
  voiceinput_run_cli smoke macos --audio-file "$audio_file"
}

voiceinput_linux_smoke_impl() {
  local audio_file=""
  local backend="ibus"
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
        echo "不支持的参数：$1" >&2
        exit 2
        ;;
    esac
  done

  if [[ -z "$audio_file" ]]; then
    echo "用法：scripts/voiceinput.sh linux smoke --audio-file /path/to/audio.wav [--model funasr|qwen|qwen-0.6b] [--backend ibus|fcitx5]" >&2
    exit 2
  fi

  voiceinput_ensure_cargo
  voiceinput_ensure_uv
  voiceinput_ensure_linux_dev_deps
  voiceinput_refresh_cargo_path
  cd "$REPO_ROOT"
  voiceinput_run_cli_linux smoke linux --audio-file "$audio_file" --backend "$backend"
}

voiceinput_windows_smoke_impl() {
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
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh windows smoke --audio-file /path/to/audio.wav [--model funasr|qwen|qwen-0.6b]

说明：
  - 当前 Windows 路径会运行本地 ASR，并优先直接注入文本
  - 如果直接注入失败，会回退到剪贴板粘贴
  - 常驻热键和 TSF/COM 注入尚未接入
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
    echo "用法：scripts/voiceinput.sh windows smoke --audio-file /path/to/audio.wav [--model funasr|qwen|qwen-0.6b]" >&2
    exit 2
  fi

  voiceinput_ensure_cargo
  voiceinput_ensure_uv
  cd "$REPO_ROOT"
  voiceinput_run_cli smoke windows --audio-file "$audio_file"
}

voiceinput_windows_install_impl() {
  local audio_file=""
  local run_smoke_after_bootstrap=false
  local run_live_app_after_bootstrap=true
  local -a bootstrap_args=()

  while [[ $# -gt 0 ]]; do
    case "$1" in
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
      --model|--backend|--model-id|--source-url|--local-dir|--revision|--device|--cuda-wheel-index|--install-cuda|--skip-existing)
        if [[ $# -lt 2 ]]; then
          echo "缺少 $1 的值" >&2
          exit 2
        fi
        if [[ "$1" == "--model" ]]; then
          if ! voiceinput_expand_model_args passthrough "$2"; then
            echo "不支持的模型：$2" >&2
            exit 2
          fi
          bootstrap_args+=("${VOICEINPUT_EXPANDED_MODEL_ARGS[@]}")
        else
          bootstrap_args+=("$1" "$2")
        fi
        shift 2
        ;;
      --no-launch)
        run_live_app_after_bootstrap=false
        shift
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh windows install [--model funasr|qwen|qwen-0.6b] [--audio-file /path/to/audio.wav]

说明：
  - 默认先执行 bootstrap，准备 Python 环境并下载模型
  - 然后启动 Windows 常驻版
  - 如果传入 --audio-file，会在准备完成后自动跑一次 Windows smoke
  - 默认会读取 config/voiceinput.env；如果要换文件，可以设置 VOICEINPUT_CONFIG_FILE
  - 当前常驻版支持全局热键、麦克风录音和文本直接注入 / 剪贴板回退
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
  if ((${#bootstrap_args[@]} > 0)); then
    voiceinput_run_bootstrap_args "${bootstrap_args[@]}"
  else
    voiceinput_run_bootstrap_args
  fi

  if [[ "$run_smoke_after_bootstrap" == true ]]; then
    voiceinput_run_platform_smoke windows "$audio_file"
  fi

  if [[ "$run_live_app_after_bootstrap" != true ]]; then
    return 0
  fi

  voiceinput_run_platform_live windows
}

voiceinput_macos_install_impl() {
  local audio_file=""
  local run_smoke_before_launch=false
  local -a bootstrap_args=()

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --backend|--model|--model-id|--source-url|--local-dir|--revision|--device|--cuda-wheel-index|--install-cuda|--skip-existing)
        if [[ "$1" == "--model" ]]; then
          if [[ $# -lt 2 ]]; then
            echo "缺少 $1 的值" >&2
            exit 2
          fi
          if ! voiceinput_expand_model_args passthrough "$2"; then
            echo "不支持的模型：$2" >&2
            exit 2
          fi
          bootstrap_args+=("${VOICEINPUT_EXPANDED_MODEL_ARGS[@]}")
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
        run_smoke_before_launch=true
        shift 2
        ;;
      --skip-smoke)
        run_smoke_before_launch=false
        audio_file=""
        shift
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh macos install [ASR 部署参数...] [--audio-file /path/to/audio.wav]

说明：
  - 先创建 Python 环境并下载本地模型
  - 再启动 macOS 常驻 app
  - 默认不再做系统输入法注册，也不再依赖重新登录
  - 如果传入 --audio-file，会在启动前先运行一次 smoke 验证
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
  if ((${#bootstrap_args[@]} > 0)); then
    voiceinput_run_bootstrap_args "${bootstrap_args[@]}"
  else
    voiceinput_run_bootstrap_args
  fi

  if [[ "$run_smoke_before_launch" == true ]]; then
    voiceinput_run_platform_smoke macos "$audio_file"
  fi

  voiceinput_run_platform_live macos
}

voiceinput_linux_install_impl() {
  local backend="ibus"
  local audio_file=""
  local run_smoke_after_bootstrap=false
  local run_live_app_after_bootstrap=true
  local -a deploy_args=()

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
        if ! voiceinput_expand_model_args passthrough "$2"; then
          echo "不支持的模型：$2" >&2
          exit 2
        fi
        deploy_args+=("${VOICEINPUT_EXPANDED_MODEL_ARGS[@]}")
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
  if ((${#deploy_args[@]} > 0)); then
    voiceinput_run_bootstrap_args "${deploy_args[@]}"
  else
    voiceinput_run_bootstrap_args
  fi

  if [[ "$run_smoke_after_bootstrap" == true ]]; then
    voiceinput_run_platform_smoke linux "$audio_file" "$backend"
    exit 0
  fi

  if [[ "$run_live_app_after_bootstrap" == true ]]; then
    voiceinput_run_platform_live linux "$backend"
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
  - 打包 macOS 常驻 app
  - 默认输出到 dist/VoiceInput.app
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
  local dist_dir="${DIST_DIR:-dist}"
  local app_bundle="$dist_dir/$app_name.app"
  local contents_dir="$app_bundle/Contents"
  local macos_dir="$contents_dir/MacOS"
  local resources_dir="$contents_dir/Resources"
  local plist_file="$contents_dir/Info.plist"
  local app_bin_name="voice-input-macos-app"
  local app_bin_path="target/release/$app_bin_name"
  local icon_source="/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericSpeaker.icns"
  local icon_name="VoiceInput.icns"

  echo "正在编译 macOS 常驻 app"
  cargo build -p voice-input-macos --bin "$app_bin_name" --release

  echo "正在组装 macOS 常驻 app：$app_bundle"
  rm -rf "$app_bundle"
  mkdir -p "$macos_dir" "$resources_dir"
  cp "$app_bin_path" "$macos_dir/$app_name"
  cp "$icon_source" "$resources_dir/$icon_name"

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
}

voiceinput_dev_install_macos_impl() {
  local -a install_args=("$@")

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --help|-h)
        cat <<'EOF'
用法：
  scripts/voiceinput.sh macos dev-install [ASR 部署参数...] [--audio-file /path/to/audio.wav]

说明：
  - 这个命令会复用 macos install 的默认 app 模式
  - 适合开发时一次完成依赖准备、模型部署和 app 启动
EOF
        exit 0
        ;;
      *)
        echo "不支持的参数：$1" >&2
        exit 2
        ;;
    esac
  done

  if ((${#install_args[@]} > 0)); then
    voiceinput_macos_install_impl "${install_args[@]}"
  else
    voiceinput_macos_install_impl
  fi
}

voiceinput_linux_dev_streaming_impl() {
  local run_prepare=false
  local restart_server=false
  local stop_server=false
  local -a app_args=()

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
    voiceinput_run_cli_linux live linux "${app_args[@]}"
}

usage() {
  cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh <command> [args...]

主命令：
  bootstrap              准备 Python 环境、安装依赖并下载模型
  model                  写入仓库级默认模型配置

平台子命令：
  macos install          准备依赖、下载模型并启动 macOS 常驻 app
  macos package          打包 macOS 常驻 app
  macos smoke            运行 macOS smoke
  macos dev-install      开发时准备依赖、下载模型并启动 app
  linux install          安装并启动 Linux 常驻版
  linux smoke            运行 Linux smoke
  linux dev              启动 Linux 开发常驻服务
  linux dev-streaming    启动 Linux FunASR 流式开发服务
  windows install        安装并启动 Windows 常驻版
  windows smoke          运行 Windows smoke

说明：
  - 所有子命令都会继续兼容现有脚本参数
  - 默认配置来自 config/voiceinput.env
  - 脚本内部会统一转调到 `voice-input-cli`
  - 也可以直接运行 `cargo run -p voice-input-cli -- <smoke|live> <macos|linux|windows> ...`
EOF
}

cmd="${1:-}"
if [[ -z "$cmd" || "$cmd" == "--help" || "$cmd" == "-h" ]]; then
  usage
  exit 0
fi

shift || true

if [[ "$cmd" == "macos" || "$cmd" == "linux" || "$cmd" == "windows" ]]; then
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
  macos-package)
    voiceinput_package_macos_impl "$@"
    ;;
  macos-smoke)
    voiceinput_macos_smoke_impl "$@"
    ;;
  linux-install)
    voiceinput_linux_install_impl "$@"
    ;;
  windows-install)
    voiceinput_windows_install_impl "$@"
    ;;
  linux-smoke)
    voiceinput_linux_smoke_impl "$@"
    ;;
  windows-smoke)
    voiceinput_windows_smoke_impl "$@"
    ;;
  macos-dev-install)
    voiceinput_dev_install_macos_impl "$@"
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
